use super::*;
use crate::shell::session_scope::Key;
use crate::{
    conversation::live::{Binding, Event, View as Chat},
    preview::Page,
    resources::SideResource,
    settings::Section,
};
#[cfg(test)]
use gpui_kit::component::*;
use sailry_protocol::{Session, SessionId};
mod children;
mod draft;
mod references;
#[cfg(test)]
pub(super) mod tests;

#[derive(Default)]
pub(crate) struct State {
    draft: Option<Entity<Chat>>,
    pub(crate) views: BTreeMap<(NodeId, SessionId), Entity<Chat>>,
}

impl Shell {
    pub(crate) fn open_dictation_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Page::Settings, window, cx);
        self.settings_target = None;
        self.settings
            .update(cx, |settings, cx| settings.select(Section::Dictation, cx));
    }

    fn open_conversation_diff(
        &mut self,
        worktree: sailry_protocol::WorktreeId,
        path: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.live.as_ref().map(|live| live.selected) else {
            return;
        };
        self.open_git_resource((node, worktree), Some(path), window, cx);
    }

    pub(crate) fn open_conversation_file(
        &mut self,
        scope: (NodeId, sailry_protocol::WorktreeId),
        path: String,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_documents(scope, Some((path, line)), window, cx);
    }

    pub(crate) fn find_conversation(
        &mut self,
        _: &crate::shell::FindConversation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page == Page::Conversation
            && let Some(chat) = self.current_chat()
        {
            chat.update(cx, |chat, cx| chat.find(window, cx));
        } else {
            cx.propagate();
        }
    }

    pub(crate) fn current_chat(&self) -> Option<&Entity<Chat>> {
        self.live.as_ref()?;
        match self.session_scope.active {
            Key::Session(node, id) => self.chats.views.get(&(node, id)),
            _ => self.chats.draft.as_ref(),
        }
    }
    pub(crate) fn live_conversation_title(&self) -> SharedString {
        match self.session_scope.active {
            Key::Session(_, id) => self
                .live
                .as_ref()
                .and_then(|live| live.view.snapshot.as_ref())
                .and_then(|snapshot| snapshot.sessions.iter().find(|session| session.id == id))
                .map(crate::activity::title)
                .unwrap_or_else(|| label(id)),
            _ => tr("chat_new"),
        }
    }

    pub(crate) fn new_live_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.page != Page::Project && self.session_scope.panels.contains_key(&Key::Draft) {
            self.activate_session(Key::Draft, window, cx);
            return;
        }
        self.park_session_panel(window, cx);
        self.session_scope.open(Key::Draft);
        self.page = Page::Conversation;
        self.select_draft_project(window, cx);
        self.open_session_pane(Key::Draft, window, cx);
        self.sync_session_panel(window, cx);
        if let Some(view) = self.current_chat() {
            view.update(cx, |view, cx| view.focus(window, cx));
        }
        cx.notify();
    }

    pub(crate) fn reveal_session(
        &mut self,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if session.delegation.is_some() {
            return;
        }
        self.park_session_panel(window, cx);
        if let Some(live) = &mut self.live {
            live.project = session.project;
            live.unassigned_worktree = session.project.is_none().then_some(session.worktree);
            if let Some(project) = session.project {
                live.worktree_choices
                    .insert((live.selected, project), session.worktree);
            }
            let key = (live.selected, session.id);
            if !self.chats.views.contains_key(&key) {
                let binding = Binding {
                    client: Arc::new(live.client()),
                    defaults: Arc::new(Client::new(live.services.local.clone())),
                    runtime: live.services.runtime.clone(),
                    project: session.project,
                    worktree: Some(session.worktree),
                    host: live.name(live.selected),
                    project_name: live
                        .selected_project()
                        .map(|p| p.name.clone().into())
                        .unwrap_or_default(),
                    branch: live
                        .selected_worktree()
                        .map(|tree| tree.path.clone().into())
                        .unwrap_or_default(),
                };
                let view = self.chat_view(binding, Some(session), window, cx);
                self.chats.views.insert(key, view);
            }
            self.activate_session(Key::Session(key.0, key.1), window, cx);
        }
        cx.notify();
    }

    pub(crate) fn chat_view(
        &mut self,
        binding: Binding,
        session: Option<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Chat> {
        let ports = self.ports_for(binding.client.target(), window, cx);
        let view = cx.new(|cx| Chat::new(binding, session, window, cx));
        if let Some(ports) = ports {
            view.update(cx, |view, cx| view.bind_ports(ports, cx));
        }
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(
            &view,
            window,
            move |shell, view, event, window, cx| match event {
                Event::PluginMounted(panel) => {
                    Self::observe_plugin_conversations(panel, window, cx)
                }
                Event::Created(session) => {
                    let node = view.read(cx).binding().client.target();
                    if let Some(ports) = shell.ports_for(node, window, cx) {
                        view.update(cx, |view, cx| view.bind_ports(ports, cx));
                    }
                    crate::preferences::recent::record(node, session.id, cx);
                    let active =
                        shell.page == Page::Conversation && shell.current_chat() == Some(view);
                    let key = (view.read(cx).binding().client.target(), session.worktree);
                    shell.chats.views.insert((key.0, session.id), view.clone());
                    if shell.chats.draft.as_ref() == Some(view) {
                        shell.chats.draft = None;
                        if let Some(live) = &mut shell.live
                            && active
                            && live.selected == key.0
                        {
                            live.project = session.project;
                            live.unassigned_worktree =
                                session.project.is_none().then_some(session.worktree);
                            if let Some(project) = session.project {
                                live.worktree_choices
                                    .insert((key.0, project), session.worktree);
                            }
                        }
                        shell.promote_browser(key.0, Some(session.worktree), session.id);
                        shell.session_scope.promote(Key::Session(key.0, session.id));
                        shell.splits.update(cx, |splits, cx| {
                            splits.rename(
                                crate::panes::Target::Draft,
                                crate::panes::Target::Session(key.0, session.id),
                                cx,
                            )
                        });
                    }
                    cx.notify();
                }
                Event::Forked(session) => {
                    shell.reveal_fork(view.clone(), *session.clone(), window, cx);
                }
                Event::Removed(session) => {
                    let node = view.read(cx).binding().client.target();
                    shell.close_session_view(Key::Session(node, *session), window, cx);
                    shell.chats.views.remove(&(node, *session));
                }
                Event::UndoChanges(turn, path) => {
                    shell.focus_chat_pane(view, window, cx);
                    shell.undo_file_changes(view.clone(), *turn, path.clone(), window, cx);
                }
                Event::GitFile(worktree, path) => {
                    shell.focus_chat_pane(view, window, cx);
                    if shell.current_chat() == Some(view) {
                        shell.open_conversation_diff(*worktree, path.clone(), window, cx);
                    }
                }
                Event::File(path) | Event::FileAt(_, path) => {
                    let child = matches!(
                        &shell.side_resource,
                        Some(SideResource::Child(panel))
                            if shell.current_chat() == Some(&panel.source)
                                && panel.tabs.iter().any(|tab| &tab.view == view)
                    );
                    if !child {
                        shell.focus_chat_pane(view, window, cx);
                    }
                    if shell.current_chat() == Some(view) || child {
                        let binding = view.read(cx).binding();
                        let worktree = match event {
                            Event::FileAt(worktree, _) => Some(*worktree),
                            _ => binding.worktree,
                        };
                        if let Some(worktree) = worktree {
                            let scope = (binding.client.target(), worktree);
                            if matches!(event, Event::FileAt(_, _)) {
                                shell.open_historical_document(
                                    scope,
                                    path.clone(),
                                    None,
                                    window,
                                    cx,
                                );
                            } else {
                                shell.open_conversation_file(scope, path.clone(), None, window, cx);
                            }
                        }
                    }
                }
                Event::Artifact(worktree, file) => {
                    shell.focus_chat_pane(view, window, cx);
                    shell.open_artifact(view.clone(), *worktree, file.clone(), window, cx);
                }
                Event::ArtifactExternal(worktree, path) => {
                    let binding = view.read(cx).binding();
                    shell.open_file_in_system(
                        (binding.client.target(), *worktree),
                        path.clone(),
                        false,
                        window,
                        cx,
                    );
                }
                Event::DirectoryAt(worktree, path) => {
                    let node = view.read(cx).binding().client.target();
                    shell.open_file_in_system((node, *worktree), path.clone(), true, window, cx);
                }
                Event::Settings(node) => {
                    shell.navigate(Page::Settings, window, cx);
                    shell.settings_target = Some(*node);
                    shell
                        .settings
                        .update(cx, |settings, cx| settings.select(Section::Providers, cx));
                }
                Event::DictationSettings => {
                    shell.open_dictation_settings(window, cx);
                }
                Event::Link(link) => {
                    shell.focus_chat_pane(view, window, cx);
                    shell.open_chat_link(view, link.clone(), window, cx);
                }
                Event::Session(id) => {
                    shell.open_session_reference(view, *id, window, cx);
                }
                Event::Subagent(id) => {
                    shell.focus_chat_pane(view, window, cx);
                    shell.open_live_child(view.clone(), *id, window, cx);
                }
                Event::Host(node) => shell.select_draft_host(view, *node, window, cx),
                Event::HostPage(node) | Event::ProjectPage(node, _) => {
                    if shell
                        .live
                        .as_ref()
                        .is_none_or(|live| live.transport_for(*node).is_none())
                    {
                        crate::feedback::error("", &crate::tr("reference_unavailable"), window, cx);
                        return;
                    }
                    let source = view.clone();
                    let next = event.clone();
                    if shell.session_scope.open.is_empty()
                        && shell.guard_file_navigation(window, cx, move |_, _, cx| {
                            source.update(cx, |_, cx| cx.emit(next.clone()));
                        })
                    {
                        return;
                    }
                    shell.select_live_host(*node, window, cx);
                    if let Event::ProjectPage(_, project) = event {
                        shell.select_live_project(*project, window, cx);
                    }
                }
                Event::LocationChanged => {
                    if shell.current_chat() == Some(view) {
                        shell.restore_session_scope(cx);
                    }
                    cx.notify();
                }
                Event::AddProject(node) => {
                    if let Some(live) = &mut shell.live
                        && live.selected != *node
                    {
                        live.select(*node, cx);
                    }
                    shell.project_editor(shell.host, None, window, cx);
                }
            },
        )
        .detach();
        view
    }

    fn reveal_fork(
        &mut self,
        source: Entity<Chat>,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != Page::Conversation || self.current_chat() != Some(&source) {
            return;
        }
        let mut binding = source.read(cx).binding();
        binding.worktree = Some(session.worktree);
        binding.branch = String::new().into();
        let node = binding.client.target();
        let child = self.chat_view(binding, Some(session.clone()), window, cx);
        self.chats.views.insert((node, session.id), child);
        self.reveal_session(session, window, cx);
    }
}

fn label(id: SessionId) -> SharedString {
    rust_i18n::t!("chat_session", id = &id.to_string()[..8])
        .to_string()
        .into()
}
