use std::collections::BTreeMap;

mod activity;
pub(crate) mod closing;
pub(crate) mod frame;
mod header;
mod limits;
pub(crate) use limits::baseline as minimum_size;
mod links;
mod rail;
mod search;
pub(crate) mod session_scope;
pub(crate) mod shortcuts;
mod splits;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    input::{InputEvent, InputState, TextareaState},
    message_scroller::MessageScrollerState,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::{preview::*, settings, sidebar, tr};

actions!(
    sailry,
    [Search, FindConversation, ToggleSidebar, ToggleDetails, Quit]
);

pub fn init(cx: &mut App) {
    cx.set_app_identity("ai.sailry.desktop", &tr("app"));
    crate::plugins::init(cx);
    shortcuts::init(cx);
    #[cfg(target_os = "macos")]
    crate::app_menu::init(cx);
    // Register application overrides after fixed system and component bindings.
    crate::shortcuts::init(cx);
}

pub struct Conversation {
    pub input: Entity<TextareaState>,
    pub(crate) turns: Vec<crate::conversation::turn::Turn>,
    pub(crate) scroller: Entity<MessageScrollerState>,
    pub(crate) preview_task: Option<Task<()>>,
    pub(crate) queue: crate::conversation::queue::State,
    pub(crate) references: crate::conversation::references::State,
    pub(crate) interaction_drafts:
        BTreeMap<crate::conversation::turn::Target, crate::conversation::interaction::Draft>,
    pub options: crate::conversation::Options,
    _input_events: Subscription,
    _input_observer: Subscription,
}

pub struct Shell {
    pub page: Page,
    pub layout: Layout,
    pub host: usize,
    pub session: usize,
    pub conversations: BTreeMap<(usize, usize), Conversation>,
    pub files: crate::resources::PreviewFiles,
    pub(crate) documents: crate::plugins::documents::Store,
    pub git: crate::resources::PreviewGit,
    pub(crate) browsers: crate::browser::bridge::State,
    pub side_resource: Option<crate::resources::SideResource>,
    pub(crate) extensions: Option<crate::plugins::navigation::Navigation>,
    pub(crate) session_scope: session_scope::State,
    pub(crate) splits: Entity<crate::panes::Workspaces>,
    pub(crate) plugin_panes: crate::plugins::panes::State,
    pub(crate) project_plugins: crate::plugins::projects::State,
    pub(crate) host_metrics: Option<Entity<crate::workspace::Metrics>>,
    pub(crate) host_monitor: Option<Entity<crate::live::host::Monitor>>,
    pub(crate) focus: FocusHandle,
    pub(crate) panel_focus: FocusHandle,
    panels: Entity<ResizableState>,
    workspace_panels: Entity<ResizableState>,
    pub sidebar: sidebar::State,
    pub(crate) workspace: crate::workspace::State,
    pub(crate) live: Option<crate::live::State>,
    pub(crate) terminals: crate::live::terminal::State,
    pub(crate) project_tab: usize,
    pub(crate) project_search: Entity<InputState>,
    pub(crate) chats: crate::live::conversation::State,
    pub settings: Entity<settings::Workspace>,
    pub(crate) ports: BTreeMap<sailry_protocol::NodeId, Entity<crate::ports::Workspace>>,
    pub(crate) activity: crate::activity::State,
    pub(crate) settings_target: Option<sailry_protocol::NodeId>,
    pub(crate) resource_keys: Entity<crate::ui::keys::Bindings>,
    pub model_picker: Entity<crate::conversation::models::Picker>,
    closing: bool,
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(true, window, cx)
    }

    pub(crate) fn secondary(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(false, window, cx)
    }

    fn build(restore: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        closing::install(window, cx);
        crate::preferences::init(cx);
        cx.observe_global::<crate::preferences::Preferences>(|_, cx| cx.notify())
            .detach();
        Self::observe_live_errors(window, cx);
        Self::observe_plugin_errors(window, cx);
        Self::observe_host_errors(window, cx);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let settings = cx.new(|cx| settings::Workspace::new(window, cx));
        let model_picker = cx.new(|_| crate::conversation::models::Picker::new());
        cx.observe(&settings, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(
            &settings,
            window,
            |shell, _, _: &settings::OpenPlugins, window, cx| {
                if shell.page == Page::Plugins {
                    return;
                }
                let target = shell.settings_target;
                shell.navigate(Page::Plugins, window, cx);
                shell.settings_target = target;
            },
        )
        .detach();
        cx.subscribe(
            &settings,
            |shell, _, event: &settings::extensions::SelectHost, cx| {
                shell.settings_target = None;
                if let Some(live) = &mut shell.live {
                    live.select(event.0, cx);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(
            &model_picker,
            |shell, _, event: &crate::model_picker::Picked, cx| {
                if let Some(conversation) =
                    shell.conversations.get_mut(&(shell.host, shell.session))
                {
                    conversation.options.model = Some(event.selection.clone());
                    conversation.options.effort = Some(event.effort);
                    cx.notify();
                }
            },
        )
        .detach();
        let splits = cx.new(|_| {
            let mut spaces = crate::panes::Workspaces::new();
            if !restore {
                spaces.suspend_saving();
            }
            spaces
        });
        cx.subscribe_in(&splits, window, |shell, _, event, window, cx| {
            shell.split_event(event, window, cx)
        })
        .detach();
        cx.observe_window_activation(window, |shell, window, cx| {
            shell.read_current_session(window, cx);
            #[cfg(target_os = "macos")]
            if window.is_window_active() {
                crate::app_menu::navigation(shell.application_navigation(cx), cx);
            }
        })
        .detach();
        let project_search =
            cx.new(|cx| InputState::new(window, cx).placeholder(tr("project_records_search")));
        cx.subscribe(&project_search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let mut shell = Self {
            page: Page::Conversation,
            layout: Layout::default(),
            host: 0,
            session: 2,
            conversations: BTreeMap::new(),
            files: crate::resources::PreviewFiles::new(window, cx),
            documents: Default::default(),
            git: crate::resources::PreviewGit::new(window, cx),
            browsers: Default::default(),
            side_resource: None,
            extensions: None,
            plugin_panes: Default::default(),
            project_plugins: Default::default(),
            session_scope: Default::default(),
            splits,
            host_metrics: None,
            host_monitor: None,
            focus,
            panel_focus: cx.focus_handle(),
            panels: cx.new(|_| ResizableState::default()),
            workspace_panels: cx.new(|_| ResizableState::default()),
            sidebar: sidebar::State::default(),
            workspace: crate::workspace::State::default(),
            live: crate::live::State::attach(cx),
            terminals: Default::default(),
            project_tab: 0,
            project_search,
            chats: Default::default(),
            settings,
            ports: BTreeMap::new(),
            activity: Default::default(),
            settings_target: None,
            resource_keys: crate::ui::keys::Bindings::new(cx),
            model_picker,
            closing: false,
        };
        if shell.live.is_none() {
            shell.ensure_conversation(window, cx);
            shell
                .session_scope
                .open(session_scope::Key::Preview(shell.host, shell.session));
        }
        if restore && shell.live.is_some() {
            shell.restore_split_workspaces(window, cx);
        }
        shell
    }

    fn ensure_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.live.is_some() || self.workspace.selected_owner(self.host).is_none() {
            return;
        }
        let key = (self.host, self.session);
        self.workspace.ensure_session(key);
        let owner = self.workspace.sessions[&key].owner;
        self.conversations
            .entry((self.host, self.session))
            .or_insert_with(|| {
                let input = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .placeholder(tr("composer"))
                        .auto_grow(2, 6)
                        .submit_on_enter(true)
                });
                let input_events =
                    cx.subscribe_in(&input, window, move |this, _, event, window, cx| {
                        if matches!(event, InputEvent::PressEnter { shift: false, .. }) {
                            this.send_preview(key, window, cx);
                        }
                        cx.notify();
                    });
                let turns = if key.1 < 2 {
                    vec![crate::conversation::sample_turn(key.1)]
                } else {
                    Vec::new()
                };
                let input_observer = cx.observe_in(&input, window, move |this, _, window, cx| {
                    this.refresh_references(key, window, cx);
                });
                let references = crate::conversation::references::State::new(
                    cx.entity().downgrade(),
                    key,
                    owner,
                    window,
                    cx,
                );
                let scroller = cx.new(|cx| MessageScrollerState::new(turns.len(), cx));
                cx.observe(&scroller, |_, _, cx| cx.notify()).detach();
                Conversation {
                    input,
                    turns,
                    scroller,
                    preview_task: None,
                    queue: Default::default(),
                    references,
                    interaction_drafts: BTreeMap::new(),
                    options: crate::conversation::Options::default(),
                    _input_events: input_events,
                    _input_observer: input_observer,
                }
            });
    }

    pub fn navigate(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(page, Page::Files | Page::Git) && self.live.is_some() {
            if let Some(entry) = self.renderer_navigation(
                if page == Page::Git {
                    sailry_protocol::plugin::desktop::ResourceKind::Git
                } else {
                    sailry_protocol::plugin::desktop::ResourceKind::Documents
                },
                cx,
            ) {
                self.open_extension(entry, window, cx);
            }
            return;
        }
        if page != Page::Conversation {
            self.park_session_panel(window, cx);
        } else if self.live.is_some() {
            self.restore_session_scope(cx);
        }
        self.settings_target = None;
        if matches!(self.page, Page::Settings | Page::Plugins) && self.page != page {
            self.settings
                .update(cx, |settings, cx| settings.deactivate(cx));
        }
        let page = if self.live.is_none()
            && self.workspace.selected_owner(self.host).is_none()
            && matches!(page, Page::Conversation | Page::Project | Page::Terminal)
        {
            Page::Host
        } else {
            page
        };
        for conversation in self.conversations.values_mut() {
            conversation.references.dismiss();
        }
        if page == Page::Settings && self.page != Page::Settings {
            self.settings.update(cx, |settings, cx| {
                let section = if settings.section.is_extension() {
                    settings::Section::General
                } else {
                    settings.section
                };
                settings.select(section, cx);
            });
        }
        if page == Page::Plugins {
            self.settings.update(cx, |settings, cx| {
                if !settings.section.is_extension() {
                    settings.select(settings::Section::Market, cx);
                }
            });
        }
        if self.page != page {
            self.focus.focus(window, cx);
        }
        self.page = page;
        self.read_current_session(window, cx);
        self.sync_host_metrics(window, cx);
        if page == Page::Conversation
            || self
                .workspace
                .sessions
                .contains_key(&(self.host, self.session))
        {
            self.ensure_conversation(window, cx);
        }
        if page == Page::Terminal && self.live.is_none() {
            let owner = self.workspace.owner(self.host);
            if self
                .workspace
                .terminal
                .and_then(|key| self.workspace.terminals.get(&key))
                .is_none_or(|terminal| terminal.owner != owner)
            {
                self.workspace.terminal = self
                    .workspace
                    .terminals
                    .iter()
                    .find(|(_, terminal)| terminal.owner == owner)
                    .map(|(&key, _)| key);
            }
        }
        if page == Page::Conversation && self.live.is_none() {
            let owner = self.workspace.sessions[&(self.host, self.session)].owner;
            self.select_owner(owner, window, cx);
            self.conversations
                .get_mut(&(self.host, self.session))
                .unwrap()
                .options
                .choices[1] = self
                .workspace
                .worktrees
                .iter()
                .filter(|(_, worktree)| worktree.project == owner.project)
                .position(|(&id, _)| id == owner.worktree)
                .unwrap_or(0);
            let key = session_scope::Key::Preview(self.host, self.session);
            if self.session_scope.active != key {
                self.park_session_panel(window, cx);
                self.session_scope.open(key);
            }
        }
        self.sync_session_panel(window, cx);
        cx.notify();
    }

    pub fn select_host(&mut self, host: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.host = host;
        self.session = 0;
        self.files = crate::resources::PreviewFiles::new(window, cx);
        self.git = crate::resources::PreviewGit::new(window, cx);
        self.close_resource_panel(cx);
        self.navigate(Page::Host, window, cx);
    }

    pub fn title(&self) -> SharedString {
        if let Some(live) = &self.live {
            if self.page == Page::Conversation {
                return self.live_conversation_title();
            }
            if self.page == Page::Terminal {
                return tr("terminal");
            }
            if self.page == Page::Host {
                return live.name(live.selected);
            }
            if self.page == Page::Project {
                return live
                    .selected_project()
                    .map(|project| project.name.clone().into())
                    .unwrap_or_else(|| tr("projects"));
            }
        }
        match self.page {
            Page::Host => {
                return tr(if self.host == 0 {
                    "local_host_name"
                } else {
                    "remote_host_name"
                });
            }
            Page::Project => {
                return self.workspace.projects[&self.workspace.owner(self.host).project]
                    .name
                    .clone();
            }
            Page::Terminal => {
                if let Some(terminal) = self
                    .workspace
                    .terminal
                    .and_then(|key| self.workspace.terminals.get(&key))
                {
                    return terminal.title.clone();
                }
            }
            Page::Conversation => {
                if let Some(session) = self.workspace.sessions.get(&(self.host, self.session)) {
                    return session.title.clone();
                }
            }
            _ => {}
        }
        tr(self.page.key())
    }

    pub(super) fn toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let available = match self.plugin_workspace(cx) {
            Some(workspace) => workspace.read(cx).has_navigation(),
            None => self.feature_navigation(window, cx).is_some(),
        };
        if !available {
            return;
        }
        self.panels.update(cx, |panels, cx| {
            if panels.sizes().len() > 1 {
                // Keep the main pane flexible so offcanvas motion does not rescale saved columns.
                panels.reset_panel(1, cx);
            }
        });
        self.layout.sidebar_open = !self.layout.sidebar_open;
        cx.notify();
    }

    fn toggle_details(&mut self, _: &ToggleDetails, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_resource_panel(cx) {
            return;
        }
        if let Some(workspace) = self.plugin_workspace(cx) {
            workspace.update(cx, |workspace, cx| {
                workspace.open = !workspace.open;
                cx.notify();
            });
            return;
        }
        if self.page == Page::Host {
            let index = self.page.panel_index();
            self.layout.panel_open[index] = !self.layout.panel_open[index];
            self.sync_host_metrics(window, cx);
            cx.notify();
            return;
        }
        if self.page == Page::Conversation {
            if self.side_resource.is_some() && !self.layout.panel_open[0] {
                self.layout.panel_open[0] = true;
                cx.notify();
            } else if self.side_resource.is_some() {
                self.close_file_panel(window, cx);
            } else {
                let plugins = self.current_chat().cloned().map(|source| {
                    let panel = cx.new(|cx| crate::plugins::Panel::new(source, cx));
                    Self::observe_plugin_conversations(&panel, window, cx);
                    cx.observe(&panel, |_, _, cx| cx.notify()).detach();
                    panel
                });
                self.side_resource = Some(crate::resources::SideResource::Launcher(plugins));
                self.layout.panel_open[0] = true;
                cx.notify();
            }
            return;
        }
        let ix = self.page.panel_index();
        self.layout.panel_open[ix] = !self.layout.panel_open[ix];
        cx.notify();
    }

    pub(crate) fn open_resource_panel(
        &mut self,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if page == Page::Git {
            if self.live.is_some() {
                self.open_destination(crate::resources::launcher::Destination::Review, window, cx);
            } else {
                self.side_resource = Some(crate::resources::SideResource::PreviewGit(Box::new(
                    crate::resources::PreviewGit::new(window, cx),
                )));
                self.layout.panel_open[0] = true;
                cx.notify();
            }
            return;
        }
        if page == Page::Files {
            if self.live.is_some() {
                self.open_destination(crate::resources::launcher::Destination::Files, window, cx);
            } else {
                self.side_resource = Some(crate::resources::SideResource::PreviewFiles(Box::new(
                    crate::resources::PreviewFiles::new(window, cx),
                )));
                self.layout.panel_open[0] = true;
                cx.notify();
            }
        }
    }

    pub(crate) fn close_resource_panel(&mut self, cx: &mut Context<Self>) {
        self.side_resource = None;
        self.layout.panel_open[0] = false;
        cx.notify();
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = (self.page == Page::Conversation
            || self.page == Page::Terminal && self.terminal_workspace_visible(cx))
        .then(|| self.splits.read(cx).active)
        .flatten();
        self.clear_plugin_focus(active, cx);
        self.sync_project_plugins(window, cx);
        self.bind_browsers(window, cx);
        self.sync_extensions(window, cx);
        #[cfg(target_os = "macos")]
        if window.is_window_active() {
            crate::app_menu::navigation(self.application_navigation(cx), cx);
        }
        self.bind_activity(window, cx);
        self.sync_plugin_activity(cx);
        self.sync_session_panel(window, cx);
        self.sync_conversation_panel(window, cx);
        self.sync_resource_shortcuts(cx);
        self.bind_host_monitor(window, cx);
        let content = self.page_content(window, cx);
        let (frame, visible, panel_width) = if let Some(workspace) = self.plugin_workspace(cx) {
            let owner = cx.entity().downgrade();
            workspace.update(cx, |state, cx| state.bind(owner, cx));
            let geometry = self.frame_geometry(
                workspace.read(cx).has_navigation(),
                self.has_resource_panel(cx),
                window,
                cx,
            );
            (content, geometry.visible, geometry.panel_width)
        } else {
            let (frame, visible, panel_width) = self.frame(content, None, window, cx);
            (frame.into_any_element(), visible, panel_width)
        };
        if let Some(browser) = self
            .side_resource
            .as_ref()
            .and_then(|panel| panel.browser(cx))
        {
            let active = matches!(&self.side_resource, Some(crate::resources::SideResource::Plugin(panel)) if panel.read(cx).resource_active());
            browser.update(cx, |browser, cx| {
                browser.visibility(
                    visible && active && self.page == Page::Conversation,
                    window,
                    cx,
                )
            });
        }
        let mut key_context = KeyContext::default();
        window.set_notification_insets(
            gpui_kit::Edges {
                right: if visible { px(panel_width) } else { px(0.) },
                ..Default::default()
            },
            cx,
        );
        key_context.add("Sailry");
        if self.page == Page::Conversation {
            key_context.add("SailryConversation");
            key_context.add(self.resource_keys.read(cx).context());
        }
        v_flex()
            .id("sailry-shell")
            .relative()
            .key_context(key_context)
            .track_focus(&self.focus)
            .size_full()
            .bg(transparent_black())
            .children(crate::theme::background("background.window", cx))
            .text_color(cx.theme().foreground)
            .text_sm()
            .on_action(cx.listener(Self::search))
            .on_action(cx.listener(Self::open_page))
            .on_action(cx.listener(Self::refresh_host))
            .on_action(cx.listener(Self::close_active_terminal))
            .on_action(cx.listener(Self::find_conversation))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_details))
            .on_action(cx.listener(Self::workspace_action))
            .on_action(cx.listener(Self::live_resource_action))
            .on_action(cx.listener(Self::start_preview_cli))
            .on_action(cx.listener(Self::document_path_action))
            .when(cfg!(target_os = "macos"), |element| {
                #[cfg(target_os = "macos")]
                {
                    crate::app_menu::bind(element, cx)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    element
                }
            })
            .on_action(cx.listener(Self::open_resource_shortcut))
            .child(div().flex_1().min_h_0().child(frame))
            .children(self.project_plugin_overlays(window, cx))
    }
}

#[cfg(test)]
pub(crate) mod tests;
