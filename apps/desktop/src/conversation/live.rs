//! Live conversation presentation; Client owns ordering and recovery, Node owns execution.
use crate::tr;
use gpui_kit::component::{
    input::{InputEvent, TextareaState},
    message_scroller::{MessageScroller, MessageScrollerState},
    *,
};
use gpui_kit::*;
use sailry_client::{Client, View as NodeView, conversation::View as History};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, Effort, NodeId, Output, ProjectId, Request, Session, SessionConfig, SessionId, TurnId,
    WorktreeId,
    conversation::{Provider, Status},
};
use std::{cell::Cell, collections::BTreeMap, rc::Rc, sync::Arc};

mod actions;
mod activity;
mod approvals;
mod assets;
mod attachments;
mod cards;
mod commands;
mod compaction;
mod composer;
mod composer_options;
mod configuration;
mod connection;
mod context;
mod contributions;
mod controls;
mod documents;
mod draft;
mod export;
mod forks;
mod loading;
mod location;
mod messages;
pub(crate) use messages::failure_key;
mod mode;
mod model_controls;
mod navigation;
mod options;
mod outgoing;
mod paging;
mod permission;
pub(crate) mod recovery;
mod repository;
mod sessions;
pub(crate) use options::{ComposerOptions, Mentions};
mod diagnostic;
mod dictation;
mod feedback;
mod questions;
mod queue;
mod references;
mod resend;
mod rewind;
pub(crate) mod search;
mod selection;
pub(crate) mod subagents;
mod submission;
#[cfg(test)]
pub(crate) mod tests;
pub(super) mod tools;
mod watch;

#[derive(Clone)]
pub(crate) struct Binding {
    pub client: Arc<Client>,
    pub defaults: Arc<Client>,
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub project: Option<ProjectId>,
    pub worktree: Option<WorktreeId>,
    pub host: SharedString,
    pub project_name: SharedString,
    pub branch: SharedString,
}

#[derive(Clone)]
pub(crate) enum Event {
    PluginMounted(Entity<crate::plugins::Panel>),
    Created(Box<Session>),
    Removed(SessionId),
    Forked(Box<Session>),
    NewConversation(String),
    UndoChanges(TurnId, Option<String>),
    GitFile(WorktreeId, Option<String>),
    File(String),
    FileAt(WorktreeId, String),
    DirectoryAt(WorktreeId, String),
    Artifact(WorktreeId, sailry_protocol::tool::File),
    Document(sailry_protocol::artifact::Summary),
    ArtifactExternal(WorktreeId, String),
    ArtifactEdit(WorktreeId, String),
    Settings(NodeId),
    DictationSettings,
    Link(Option<WorktreeId>, SharedString),
    Subagent(SessionId),
    Session(SessionId),
    Host(NodeId),
    HostPage(NodeId),
    ProjectPage(NodeId, ProjectId),
    AddProject(NodeId),
    LocationChanged,
}

pub(crate) struct View {
    contributions: Entity<crate::plugins::contributions::Registry>,
    contribution_metadata: Entity<crate::plugins::metadata::Metadata>,
    sidebar: bool,
    external_header: bool,
    compact_composer: bool,
    icon_context: bool,
    composer_settings: Entity<composer::Panel>,
    composer_options: ComposerOptions,
    resource: Option<sailry_protocol::connection::Resource>,
    assistant: Option<sailry_protocol::plugin::conversation::Binding>,
    binding: Binding,
    hosts: Vec<(NodeId, SharedString)>,
    git: bool,
    draft_mode: Option<sailry_protocol::WorkMode>,
    draft_permission: Option<sailry_protocol::Permission>,
    preparing: Option<sailry_protocol::conversation::Input>,
    session: Option<Session>,
    node: NodeView,
    defaults: NodeView,
    history: History,
    python: sailry_client::python::View,
    config: Option<SessionConfig>,
    config_owner: NodeId,
    input: Entity<TextareaState>,
    restored_input: Option<gpui_kit::base::input::InputContent>,
    submissions: submission::State,
    references: references::State,
    attachments: attachments::Drafts,
    images: Entity<attachments::images::Images>,
    scroller: Entity<MessageScrollerState>,
    message_display: crate::preferences::MessageDisplay,
    message_text_size: u8,
    rows: Vec<TurnId>,
    outgoing: Option<outgoing::Message>,
    sent: BTreeMap<TurnId, outgoing::Message>,
    navigation: navigation::Navigation,
    expanded: BTreeMap<(TurnId, String), bool>,
    changes: BTreeMap<TurnId, Entity<messages::changes::Card>>,
    editing: Option<messages::Editing>,
    texts: std::cell::RefCell<BTreeMap<String, Entity<crate::content::markdown::State>>>,
    prompts: std::cell::RefCell<BTreeMap<TurnId, Entity<super::prompt::State>>>,
    cards: std::cell::RefCell<
        BTreeMap<sailry_protocol::plugin::card::Receipt, Entity<crate::plugins::cards::Card>>,
    >,
    selection: Option<crate::content::selection::Excerpt>,
    model_picker: Entity<super::models::Picker>,
    model_controls: Entity<model_controls::Panel>,
    queue: Entity<queue::Panel>,
    commands: Entity<commands::Panel>,
    ports: Option<(Entity<crate::ports::Workspace>, Subscription)>,
    approvals: approvals::Actions,
    repository: repository::State,
    questions: questions::Form,
    search: Entity<search::Search>,
    assets: Entity<assets::Assets>,
    controls: Entity<controls::Controls>,
    reveal: Option<search::Target>,
    provider_ids: BTreeMap<(NodeId, sailry_protocol::ProviderId), usize>,
    dictation: dictation::State,
    pending: bool,
    guidance_pending: Rc<Cell<bool>>,
    queue_edit_pending: Rc<Cell<bool>>,
    retry: Option<actions::Attempt>,
    error: Option<&'static str>,
    backup: Option<Session>,
    stop: CancellationToken,
    action: Option<Task<()>>,
    subscription: Option<Task<()>>,
    clock: Option<Task<()>>,
    paging_requested: bool,
    older: Option<tokio::sync::mpsc::Sender<sailry_client::conversation::HistoryRequest>>,
    _node: Task<()>,
    _python: Task<()>,
    defaults_stop: CancellationToken,
    _defaults: Option<Task<()>>,
    _input_events: Subscription,
    _input_observer: Subscription,
}

impl EventEmitter<Event> for View {}

impl Drop for View {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl View {
    pub(crate) fn new(
        binding: Binding,
        session: Option<Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_input(binding, session, None, window, cx)
    }

    fn with_input(
        binding: Binding,
        session: Option<Session>,
        input: Option<Entity<TextareaState>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::observe_errors(window, cx);
        crate::preferences::init(cx);
        cx.observe_global::<crate::preferences::Preferences>(|view, cx| {
            let display = crate::preferences::data(cx)
                .message_display
                .unwrap_or_default();
            let text_size = crate::preferences::data(cx).message_text_size();
            if view.message_display != display || view.message_text_size != text_size {
                view.message_display = display;
                view.message_text_size = text_size;
                view.scroller
                    .update(cx, |scroller, cx| scroller.remeasure(cx));
                cx.notify();
            }
        })
        .detach();
        let input = input.unwrap_or_else(|| {
            cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(tr("composer"))
                    .auto_grow(2, 6)
                    .submit_on_enter(true)
            })
        });
        let input_events = cx.subscribe_in(&input, window, |view, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                view.submissions.edited();
            }
            if matches!(event, InputEvent::PressEnter { shift: false, .. }) && !view.readonly() {
                view.send(window, cx);
            }
            cx.notify();
        });
        let input_observer = cx.observe_in(&input, window, |view, _, window, cx| {
            if !view.input.read(cx).value().is_empty() {
                view.submissions.edited();
            }
            if view.recovered_input(cx) {
                cx.notify();
                return;
            }
            view.confirm_mode_command(window, cx);
            view.capture_command(cx);
            view.refresh_references(window, cx);
            cx.notify();
        });
        let references = references::State::new(cx.entity().downgrade(), window, cx);
        let composer_source = cx.weak_entity();
        let contributions = cx
            .new(|cx| crate::plugins::contributions::Registry::new(&binding, composer_source, cx));
        let contribution_metadata = contributions.read(cx).metadata();
        cx.observe_in(&contributions, window, |view, _, window, cx| {
            view.refresh_commands(window, cx);
            cx.notify();
        })
        .detach();
        cx.subscribe(&contributions, |_, _, event, cx| {
            if let crate::plugins::contributions::Event::Mounted(panel) = event {
                cx.emit(Event::PluginMounted(panel.clone()));
            }
        })
        .detach();
        let stop = CancellationToken::new();
        let node = watch::node(&binding, false, &stop, cx);
        let python = watch::python(&binding, &stop, cx);
        let defaults_stop = stop.child_token();
        let defaults = (session.is_none() && binding.defaults.target() != binding.client.target())
            .then(|| watch::node(&binding, true, &defaults_stop, cx));
        let (history, older) = session
            .as_ref()
            .map(|session| watch::history(&binding, session.id, History::default(), &stop, cx))
            .unzip();
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let model_picker = cx.new(|_| super::models::Picker::new());
        let composer_owner = cx.entity();
        let composer_settings = cx.new(|cx| composer::Panel::new(composer_owner, cx));
        let owner = cx.weak_entity();
        let model_controls = cx.new(|cx| {
            model_controls::Panel::new(
                owner,
                model_picker.clone(),
                contributions.clone(),
                window,
                cx,
            )
        });
        let attachments = attachments::Drafts::default();
        let images = cx.new(|cx| {
            attachments::images::Images::new(
                binding.client.clone(),
                binding.runtime.clone(),
                attachments.slots.clone(),
                cx,
            )
        });
        let commands = cx.new(|_| commands::Panel::new(binding.clone()));
        cx.observe(&commands, |_, _, cx| cx.notify()).detach();
        let queue_owner = cx.weak_entity();
        let guidance_pending = Rc::new(Cell::new(false));
        let queue_edit_pending = Rc::new(Cell::new(false));
        let queue = cx.new(|cx| {
            queue::Panel::new(
                queue_owner,
                binding.clone(),
                images.clone(),
                guidance_pending.clone(),
                queue_edit_pending.clone(),
                window,
                cx,
            )
        });
        let search = cx.new(|cx| {
            search::Search::new(
                binding.clone(),
                session.as_ref().map(|session| session.id),
                window,
                cx,
            )
        });
        let owner = cx.weak_entity();
        let assets = cx.new(|cx| {
            assets::Assets::new(
                owner,
                binding.clone(),
                session.as_ref().map(|session| session.id),
                images.clone(),
                cx,
            )
        });
        let controls = cx.new(|_| controls::Controls::new(search.clone(), assets.clone()));
        cx.subscribe(&search, |view, _, event, cx| match event {
            search::Event::Reveal(target) => view.reveal_match(target.clone(), cx),
            search::Event::Reset => view.reveal = None,
        })
        .detach();
        cx.subscribe_in(
            &model_picker,
            window,
            |view, _, event: &crate::model_picker::Picked, window, cx| {
                view.select_model(&event.selection, event.effort, window, cx);
            },
        )
        .detach();
        cx.subscribe_in(
            &model_picker,
            window,
            |view, _, event: &crate::model_picker::EffortPicked, window, cx| {
                view.select_effort(event.0, window, cx);
            },
        )
        .detach();
        cx.observe(&scroller, |_, _, cx| cx.notify()).detach();
        let remembered = crate::preferences::data(cx).composer.filter(|selection| {
            selection
                .model
                .as_ref()
                .is_some_and(|model| model.source == binding.client.target())
        });
        crate::updater::recovery::register(window, cx);
        Self {
            contributions,
            contribution_metadata,
            sidebar: false,
            external_header: false,
            compact_composer: false,
            icon_context: false,
            composer_settings,
            composer_options: ComposerOptions::default(),
            resource: session.as_ref().and_then(|session| session.config.resource),
            assistant: session
                .as_ref()
                .and_then(|session| session.config.assistant.clone()),
            config: session.as_ref().map(|session| session.config.clone()),
            config_owner: binding.client.target(),
            binding,
            hosts: Vec::new(),
            git: false,
            draft_mode: remembered.as_ref().map(|selection| selection.mode),
            draft_permission: remembered.as_ref().map(|selection| selection.permission),
            preparing: None,
            session,
            node: NodeView::default(),
            defaults: NodeView::default(),
            history: History::default(),
            python: sailry_client::python::View::default(),
            input,
            restored_input: None,
            submissions: submission::State::default(),
            references,
            attachments,
            images,
            scroller,
            message_display: crate::preferences::data(cx)
                .message_display
                .unwrap_or_default(),
            message_text_size: crate::preferences::data(cx).message_text_size(),
            rows: Vec::new(),
            outgoing: None,
            sent: BTreeMap::new(),
            navigation: navigation::Navigation::new(cx),
            expanded: BTreeMap::new(),
            changes: BTreeMap::new(),
            editing: None,
            texts: Default::default(),
            prompts: Default::default(),
            cards: Default::default(),
            selection: None,
            model_picker,
            model_controls,
            queue,
            commands,
            ports: None,
            approvals: Default::default(),
            repository: Default::default(),
            questions: questions::Form::new(cx),
            search,
            assets,
            controls,
            reveal: None,
            provider_ids: BTreeMap::new(),
            dictation: dictation::State::default(),
            pending: false,
            guidance_pending,
            queue_edit_pending,
            retry: None,
            error: None,
            backup: None,
            stop,
            action: None,
            subscription: history,
            clock: None,
            older,
            paging_requested: false,
            _node: node,
            _python: python,
            defaults_stop,
            _defaults: defaults,
            _input_events: input_events,
            _input_observer: input_observer,
        }
    }

    pub(crate) fn session(&self) -> Option<SessionId> {
        self.session.as_ref().map(|session| session.id)
    }

    pub(crate) fn summary(&self) -> Option<&Session> {
        self.node
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .sessions
                    .iter()
                    .find(|session| Some(session.id) == self.session())
            })
            .or(self.session.as_ref())
    }

    pub(crate) fn binding(&self) -> Binding {
        self.binding.clone()
    }

    pub(crate) fn node_snapshot(&self) -> Option<&sailry_protocol::Snapshot> {
        self.node
            .snapshot
            .as_ref()
            .filter(|snapshot| snapshot.node == self.binding.client.target())
    }

    pub(crate) fn composer_focused(&self, window: &Window, cx: &App) -> bool {
        self.input.read(cx).focus_handle(cx).is_focused(window)
    }

    pub(crate) fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_repository(repository::Refresh::Focus, cx);
        if self.readonly() {
            return;
        }
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn readonly(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.delegation.is_some())
    }

    pub(crate) fn header_controls(&self) -> Entity<controls::Controls> {
        self.controls.clone()
    }

    #[cfg(feature = "workload-tests")]
    pub(crate) fn loaded_turns(&self) -> usize {
        self.rows.len()
    }

    #[cfg(feature = "workload-tests")]
    pub(crate) fn completed_turns(&self) -> Option<u64> {
        let snapshot = self.history.snapshot.as_ref()?;
        snapshot
            .page
            .runs
            .iter()
            .all(|run| run.status == Status::Completed)
            .then_some(snapshot.statistics.turns)
    }

    pub(crate) fn find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |search, cx| search.open(window, cx));
    }

    /// The running task that accepts guidance at its next model boundary.
    fn recipient(&self) -> Option<&sailry_protocol::conversation::Run> {
        self.history
            .snapshot
            .as_ref()?
            .page
            .runs
            .iter()
            .find(|run| {
                run.status == Status::Running
                    && run.kind == sailry_protocol::conversation::RunKind::Task
            })
    }

    fn active_run(&self) -> Option<&sailry_protocol::conversation::Run> {
        self.history
            .snapshot
            .as_ref()?
            .page
            .runs
            .iter()
            .find(|run| matches!(run.status, Status::Running | Status::Stopping))
    }

    fn active(&self) -> Option<TurnId> {
        self.active_run().map(|run| run.turn)
    }

    fn expand(&mut self, turn: TurnId, key: String, open: bool, cx: &mut Context<Self>) {
        self.expanded.insert((turn, key), open);
        if let Some(index) = self.rows.iter().position(|item| *item == turn) {
            self.scroller.update(cx, |scroller, cx| {
                scroller.remeasure_items(index..index + 1, cx);
            });
        }
        cx.notify();
    }

    pub(crate) fn connected(&self) -> bool {
        self.node.connected
            && self.configuration().connected
            && (self.session.is_none() || self.history.connected)
    }

    fn configured(&self) -> bool {
        (self.config_owner == self.binding.client.target()
            || self.config_owner == self.binding.defaults.target())
            && self.config.as_ref().is_some_and(|config| {
                self.providers().any(|provider| {
                    provider.enabled
                        && provider.id == config.provider
                        && provider.models.iter().any(|model| model.id == config.model)
                })
            })
    }
}

impl Render for View {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::prelude::FluentBuilder as _;

        self.sync_question(window, cx);
        self.contributions.update(cx, |registry, cx| {
            registry.sync(
                &self.binding,
                crate::plugins::contributions::ViewState {
                    session: self.session.as_ref(),
                    assistant: self.assistant.as_ref(),
                    snapshot: self.node.snapshot.as_ref(),
                    connected: self.connected(),
                    credentials: &[],
                },
                window,
                cx,
            )
        });
        self.search.update(cx, |search, cx| {
            search.sync(
                self.session(),
                self.history
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.page.revision),
                self.history.connected,
                window,
                cx,
            )
        });
        self.assets.update(cx, |assets, cx| {
            assets.sync(
                &self.binding,
                &self.images,
                self.session(),
                self.history.snapshot.as_ref().map(|snapshot| {
                    (
                        snapshot.page.revision,
                        snapshot
                            .page
                            .entries
                            .last()
                            .map_or(0, |entry| entry.sequence),
                    )
                }),
                window,
                cx,
            )
        });
        self.navigation.rows.borrow_mut().clear();
        let welcome = self.session.is_none() && self.rows.is_empty();
        let owner = cx.entity().downgrade();
        let body = if self.loading_history() {
            self.history_placeholder().into_any_element()
        } else if self.rows.is_empty() && self.readonly() {
            div().into_any_element()
        } else if self.rows.is_empty() && (self.resource.is_some() || self.assistant.is_some()) {
            crate::empty_state::panel(IconName::Bot, "chat_sidebar_empty", cx).into_any_element()
        } else if self.rows.is_empty() {
            v_flex()
                .size_full()
                .child(div().flex_1().min_h_0().child(self.welcome(welcome, cx)))
                .into_any_element()
        } else {
            MessageScroller::new(
                "live-message-list",
                self.scroller.clone(),
                move |index, window, cx| {
                    owner
                        .update(cx, |view, cx| {
                            // GPUI's tail-to-wheel transition counts list padding twice.
                            // Keep spacing inside measured rows so scrolling has no fixed viewport inset.
                            let content = view.message(index, window, cx);
                            let geometry = view.navigation.rows.clone();
                            let turn = view.rows[index];
                            let paging = cx.entity().downgrade();
                            v_flex()
                                .w_full()
                                .when(view.sidebar && index == 0, |row| row.pt_3())
                                .when(
                                    !view.sidebar
                                        && index == 0
                                        && view.history.snapshot.as_ref().is_none_or(|snapshot| {
                                            snapshot.page.next_before.is_none()
                                        }),
                                    |row| row.pt_6(),
                                )
                                .when(index + 1 == view.rows.len(), |row| row.pb_6())
                                .child(content)
                                .on_prepaint(move |bounds, _, cx| {
                                    geometry.borrow_mut().insert(turn, bounds);
                                    if index == 0 {
                                        let paging = paging.clone();
                                        cx.defer(move |cx| {
                                            let _ = paging
                                                .update(cx, |view, cx| view.page_at_start(cx));
                                        });
                                    }
                                })
                                .into_any_element()
                        })
                        .unwrap_or_else(|_| div().into_any_element())
                },
            )
            .with_list_style(StyleRefinement::default().px_0().py_0())
            .with_jump_button_label(tr("turn_latest"))
            .with_jump_button_renderer(|button| {
                button
                    .accessibility_label(tr("turn_latest"))
                    .debug_selector(|| "live-history-latest".into())
            })
            .into_any_element()
        };
        let show_navigation =
            !self.rows.is_empty() && !self.readonly() && self.navigation.visible();
        let navigation_bounds = self.navigation.bounds.clone();
        let owner = cx.weak_entity();
        v_flex()
            .key_context("LiveConversation")
            .debug_selector(|| "live-conversation".into())
            .relative()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(|view, _, _, cx| view.dismiss_selection(cx)))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && view.selection.is_some() {
                    view.dismiss_selection(cx);
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .min_h_0()
            .children(crate::theme::effects::layer("conversation", cx))
            .when(
                self.sidebar && self.assistant.is_some() && !self.external_header,
                |body| body.child(self.session_controls(cx)),
            )
            .children(crate::theme::banner(
                if welcome {
                    "banner.new_session"
                } else {
                    "banner.conversation.top"
                },
                cx,
            ))
            .child(
                div()
                    .debug_selector(|| "live-history-viewport".into())
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .on_prepaint(move |bounds, _, cx| {
                        // Paging still needs geometry when the floating navigation rail is absent.
                        if !show_navigation {
                            navigation_bounds.set(bounds);
                        }
                        let owner = owner.clone();
                        cx.defer(move |cx| {
                            let _ = owner.update(cx, |view, cx| {
                                let compact = bounds.size.width < px(680.);
                                let icons = bounds.size.width < px(440.);
                                let changed =
                                    view.compact_composer != compact || view.icon_context != icons;
                                view.compact_composer = compact;
                                view.icon_context = icons;
                                if view.navigation.resize(bounds.size.width) || changed {
                                    cx.notify();
                                }
                            });
                        });
                    })
                    .child(div().size_full().child(body))
                    .when(show_navigation, |area| {
                        area.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .child(self.navigation(window, cx)),
                        )
                    }),
            )
            .children(
                (!welcome)
                    .then(|| crate::theme::banner("banner.conversation.bottom", cx))
                    .flatten(),
            )
            .when(
                !self.readonly()
                    && (self.resource.is_some()
                        || self.assistant.is_some()
                        || self.session.is_some()
                        || !self.rows.is_empty()),
                |body| body.child(self.composer(cx)),
            )
            .children(self.contributions.read(cx).overlays(window, cx))
            .children(self.selection_toolbar(cx))
    }
}
