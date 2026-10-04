//! Plugin assistants use captured scopes and ordinary Node-owned sessions.
use crate::conversation::live::{Binding, ComposerOptions, Event, Mentions, View as Chat};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{component::*, *};
use gpui_shell::{HostModule, HostValue};
use sailry_protocol::{Session, connection::Resource, plugin};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
mod observations;

pub(super) struct Scope {
    pub owner: WeakEntity<super::Panel>,
    pub binding: Binding,
    pub package: plugin::Reference,
    pub snapshot: super::panel::Snapshot,
    pub declarations: Vec<plugin::conversation::Declaration>,
    pub source: Option<Entity<Chat>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Request {
    assistant: Option<String>,
    resource: Option<Resource>,
}

impl Request {
    fn parse(props: &HostValue) -> Option<Self> {
        let assistant = match props.get("assistant") {
            None => None,
            Some(value) => Some(value.as_str()?.to_owned()),
        };
        let resource = match props.get("resource") {
            None => None,
            Some(value) => {
                let id = value.get("id")?.as_str()?;
                Some(match value.get("kind")?.as_str()? {
                    "database" => Resource::Database(id.parse().ok()?),
                    "ssh" => Resource::Ssh(id.parse().ok()?),
                    _ => return None,
                })
            }
        };
        if resource.is_some() && assistant.is_none() {
            return None;
        }
        Some(Self {
            assistant,
            resource,
        })
    }
}

impl Scope {
    fn resolve(
        &self,
        request: &Request,
    ) -> Option<(Binding, Option<Session>, plugin::conversation::Binding)> {
        let snapshot = self.snapshot.borrow();
        let snapshot = snapshot.as_ref()?;
        if snapshot.node != self.binding.client.target()
            || !snapshot
                .plugins
                .iter()
                .any(|package| package.enabled && package.reference() == self.package)
        {
            return None;
        }
        let id = request.assistant.as_ref()?;
        let declaration = self.declarations.iter().find(|entry| entry.id == *id)?;
        let mut binding = self.binding.clone();
        if declaration.resource != plugin::conversation::Resource::Workspace {
            binding.project = None;
            binding.worktree = None;
            binding.project_name = SharedString::default();
            binding.branch = SharedString::default();
        }
        if !declaration
            .resource
            .matches(binding.project, request.resource)
            || (binding.project.is_some() && binding.worktree.is_none())
        {
            return None;
        }
        match request.resource {
            Some(Resource::Database(id))
                if !snapshot.databases.iter().any(|profile| profile.id == id) =>
            {
                return None;
            }
            Some(Resource::Ssh(id)) if !snapshot.ssh.iter().any(|profile| profile.id == id) => {
                return None;
            }
            _ => {}
        }
        let assistant = plugin::conversation::Binding {
            package: self.package.clone(),
            id: id.clone(),
        };
        let session = snapshot
            .sessions
            .iter()
            .rev()
            .find(|session| {
                !session.archived
                    && session.delegation.is_none()
                    && session.config.assistant.as_ref() == Some(&assistant)
                    && session.config.resource == request.resource
                    && session.project == binding.project
                    && (binding.project.is_none() || Some(session.worktree) == binding.worktree)
            })
            .cloned();
        Some((binding, session, assistant))
    }
}

#[derive(Clone, Default)]
pub(super) struct Conversations(Rc<RefCell<Views>>);

type Views = BTreeMap<String, (Request, Entity<Chat>)>;

impl Conversations {
    pub fn module(
        &self,
        module: HostModule,
        scope: Option<Scope>,
        stop: sailry_link::CancellationToken,
        cx: &App,
    ) -> HostModule {
        let mounted = self.clone();
        let observations = observations::Observations::default();
        let module = observations.module(module, stop.clone());
        // Capture identity before JavaScript starts; later global navigation cannot redirect it.
        let capture = scope
            .as_ref()
            .and_then(|scope| scope.source.as_ref())
            .and_then(|source| {
                source
                    .read(cx)
                    .summary()
                    .cloned()
                    .map(|session| (source.read(cx).binding(), session, source.downgrade()))
            });
        module.component("Conversation", move |args, window, cx| {
            if stop.is_cancelled() {
                return unavailable(cx);
            }
            let Some(request) = Request::parse(args.props()) else {
                return unavailable(cx);
            };
            let existing = mounted.0.borrow().get(args.id()).cloned();
            let chat = if let Some((original, chat)) = existing {
                if original != request {
                    return unavailable(cx);
                }
                chat
            } else {
                let chat = if request.assistant.is_some() {
                    let Some((binding, session, assistant)) =
                        scope.as_ref().and_then(|scope| scope.resolve(&request))
                    else {
                        return unavailable(cx);
                    };
                    let chat = cx.new(|cx| {
                        Chat::for_assistant(
                            binding,
                            session,
                            assistant,
                            request.resource,
                            window,
                            cx,
                        )
                    });
                    let owner = scope.as_ref().unwrap().owner.clone();
                    cx.subscribe(&chat, move |source, event: &Event, cx| {
                        if matches!(
                            event,
                            Event::PluginMounted(_)
                                | Event::Settings(_)
                                | Event::DictationSettings
                                | Event::Link(_)
                                | Event::File(_)
                                | Event::FileAt(_, _)
                                | Event::DirectoryAt(_, _)
                                | Event::Artifact(_, _)
                                | Event::ArtifactExternal(_, _)
                                | Event::GitFile(_, _)
                                | Event::UndoChanges(_, _)
                                | Event::Subagent(_)
                                | Event::Session(_)
                                | Event::HostPage(_)
                                | Event::ProjectPage(_, _)
                                | Event::Forked(_)
                        ) {
                            let _ = owner.update(cx, |_, cx| {
                                cx.emit(super::ConversationEvent {
                                    source: source.clone(),
                                    action: event.clone(),
                                })
                            });
                        }
                    })
                    .detach();
                    chat
                } else {
                    let Some((binding, session, source)) = capture.as_ref() else {
                        return unavailable(cx);
                    };
                    let options = match session.config.resource {
                        Some(Resource::Database(_)) => {
                            ComposerOptions::connection(Mentions::Database)
                        }
                        Some(Resource::Ssh(_)) => {
                            ComposerOptions::connection(Mentions::Attachments)
                        }
                        None => ComposerOptions::default(),
                    };
                    let chat = cx.new(|cx| {
                        Chat::embedded(binding.clone(), Some(session.clone()), options, window, cx)
                    });
                    let source = source.clone();
                    cx.subscribe(&chat, move |_, event: &Event, cx| {
                        let _ = source.update(cx, |_, cx| cx.emit(event.clone()));
                    })
                    .detach();
                    chat
                };
                mounted
                    .0
                    .borrow_mut()
                    .insert(args.id().into(), (request.clone(), chat.clone()));
                if let Some(scope) = &scope {
                    observations.attach(args.id().into(), &scope.package.name, &chat, cx);
                }
                chat
            };
            let heading = args
                .props()
                .get("heading")
                .and_then(HostValue::as_str)
                .map(str::to_owned);
            chat.update(cx, |chat, cx| {
                chat.set_external_header(heading.is_some(), cx)
            });
            v_flex()
                .size_full()
                .min_w_0()
                .min_h_0()
                .debug_selector(|| "plugin-conversation".into())
                .when_some(heading, |body, heading| {
                    body.child(
                        crate::header::Header::new(format!("{}-header", args.id()), cx)
                            .bordered(false)
                            .child(heading)
                            .child(div().flex_1())
                            .child(chat.update(cx, |chat, cx| chat.session_controls(cx))),
                    )
                })
                .child(div().flex_1().min_w_0().min_h_0().child(chat))
                .into_any_element()
        })
    }

    #[cfg(test)]
    pub fn first(&self) -> Option<Entity<Chat>> {
        self.0
            .borrow()
            .values()
            .next()
            .map(|(_, chat)| chat.clone())
    }
}

fn unavailable(cx: &App) -> AnyElement {
    div()
        .p_4()
        .text_color(cx.theme().muted_foreground)
        .debug_selector(|| "plugin-conversation-unavailable".into())
        .child(crate::tr("plugins_conversation_unavailable"))
        .into_any_element()
}
