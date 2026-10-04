use super::{cache::Cache, host::Host, metadata::Metadata, runtime::Mounted};
use crate::conversation::live::{Binding, View as Chat};
use gpui_kit::*;
use sailry_client::View;
use sailry_link::CancellationToken;
use sailry_protocol::{
    Output,
    plugin::{Context as Scope, Reference, Summary},
};
use std::{cell::RefCell, rc::Rc};
mod loading;

pub(super) type Snapshot = Rc<RefCell<Option<sailry_protocol::Snapshot>>>;

pub(crate) struct Panel {
    pub(crate) activity_sources: super::host::activity::Sources,
    pub(crate) usage_sources: super::usage::Sources,
    pub(crate) workspace: Option<Entity<super::workspace::State>>,
    pub(crate) resource: Option<Entity<super::resource::State>>,
    browser: Option<Entity<crate::browser::Browser>>,
    pub(crate) documents: Option<Entity<super::documents::Controller>>,
    pub(crate) pending_document: Option<(String, Option<usize>)>,
    pub(crate) artifact: Option<super::previews::Resource>,
    pub(super) pane: Option<super::panes::Resource>,
    pub(super) pane_events: super::panes::Events,
    pub(super) pane_focus: bool,
    composer: Option<WeakEntity<Chat>>,
    contributions: Option<std::sync::Arc<super::contributions::bridge::Bridge>>,
    pub(super) content_sized: bool,
    pub(crate) source: Option<Entity<Chat>>,
    pub(super) surface: sailry_protocol::plugin::desktop::Surface,
    pub(super) session: Option<sailry_protocol::SessionId>,
    pub(super) binding: Binding,
    pub(super) metadata: Entity<Metadata>,
    pub(super) connected: bool,
    pub(super) source_error: Option<sailry_protocol::Fault>,
    summaries: Vec<Summary>,
    pub(super) snapshot: Snapshot,
    pub(super) selected: Option<Reference>,
    pub(super) mounted: Option<Mounted>,
    pub(super) loading: bool,
    reload: bool,
    pub(super) error: Option<&'static str>,
    pub(super) viewport: Option<Size<Pixels>>,
    stop: CancellationToken,
    load_stop: CancellationToken,
    _watch: Task<()>,
    _metadata: Subscription,
    _header: Option<Subscription>,
    _workspace: Option<Subscription>,
    load: Option<Task<()>>,
}

pub(crate) struct ConversationEvent {
    pub source: Entity<Chat>,
    pub action: crate::conversation::live::Event,
}

impl EventEmitter<ConversationEvent> for Panel {}

pub(crate) struct OpenSession {
    pub node: sailry_protocol::NodeId,
    pub session: sailry_protocol::SessionId,
}
impl EventEmitter<OpenSession> for Panel {}

impl Drop for Panel {
    fn drop(&mut self) {
        self.stop.cancel();
        self.load_stop.cancel();
    }
}

impl Panel {
    pub(crate) fn browser(
        source: Entity<Chat>,
        browser: Entity<crate::browser::Browser>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::new(source, cx);
        panel.browser = Some(browser);
        panel
    }

    pub(crate) fn browser_view(&self) -> Option<Entity<crate::browser::Browser>> {
        self.browser.clone()
    }

    pub(crate) fn with_documents(
        mut self,
        documents: Option<Entity<super::documents::Controller>>,
    ) -> Self {
        self.documents = documents;
        self
    }

    pub(crate) fn resource_active(&self) -> bool {
        self.connected
            && !self.loading
            && self.mounted.as_ref().is_some_and(Mounted::active)
            && self
                .selected
                .as_ref()
                .is_some_and(|package| self.available(package))
    }

    pub(crate) fn matches_source(&self, cx: &App) -> bool {
        let Some(source) = &self.source else {
            return false;
        };
        let binding = source.read(cx).binding();
        self.binding.client.target() == binding.client.target()
            && (((self.documents.is_some() || self.resource.is_some()) && self.session.is_none())
                || self.binding.worktree == binding.worktree
                    && self.session == source.read(cx).session())
    }

    pub(crate) fn new(source: Entity<Chat>, cx: &mut Context<Self>) -> Self {
        let binding = source.read(cx).binding();
        let session = source.read(cx).session();
        Self::bind(Some(source), binding, session, cx)
    }

    pub(crate) fn standalone(binding: Binding, cx: &mut Context<Self>) -> Self {
        Self::bind(None, binding, None, cx)
    }

    pub(crate) fn workspace(binding: Binding, details: bool, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::standalone(binding, cx);
        if details {
            let workspace = cx.new(super::workspace::State::new);
            panel.attach_workspace(workspace, cx);
        }
        panel
    }

    pub(crate) fn attach_workspace(
        &mut self,
        workspace: Entity<super::workspace::State>,
        cx: &mut Context<Self>,
    ) {
        self._workspace = Some(cx.observe(&workspace, |panel, _, cx| {
            if let Some(mounted) = &panel.mounted
                && let Ok(script) = mounted
                    .root
                    .read(cx)
                    .content()
                    .clone()
                    .downcast::<gpui_shell::ScriptView>()
            {
                script.update(cx, |_, cx| cx.notify());
            }
            cx.notify();
        }));
        self.workspace = Some(workspace);
    }

    pub(crate) fn embedded_geometry(&self, width: Pixels, cx: &mut App) {
        if let Some(workspace) = &self.workspace
            && workspace.read(cx).embedded
        {
            workspace.update(cx, |state, cx| state.resize(width, cx));
        }
    }

    pub(super) fn pane(
        binding: Binding,
        resource: super::panes::Resource,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::bind(None, binding, None, cx);
        panel.pane = Some(resource);
        panel
    }

    pub(crate) fn settings(binding: Binding, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::standalone(binding, cx);
        panel.surface = sailry_protocol::plugin::desktop::Surface::Settings;
        panel
    }

    pub(crate) fn contributions(
        binding: Binding,
        session: Option<sailry_protocol::SessionId>,
        surface: sailry_protocol::plugin::desktop::Surface,
        bridge: std::sync::Arc<super::contributions::bridge::Bridge>,
        composer: Option<WeakEntity<Chat>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self::bind(None, binding, session, cx);
        panel.surface = surface;
        panel.contributions = Some(bridge);
        panel.composer = composer;
        panel
    }

    pub(super) fn location_source(&self) -> Option<WeakEntity<Chat>> {
        if self.resource.is_some() {
            return None;
        }
        self.composer
            .clone()
            .or_else(|| self.source.as_ref().map(Entity::downgrade))
    }

    pub(crate) fn ready_for(&self, package: &Reference, cx: &App) -> bool {
        self.connected
            && self.selected.is_none()
            && self.available(package)
            && self.metadata.read(cx).settled()
    }

    pub(crate) fn ready_settings(&self, name: &str, cx: &App) -> Option<Reference> {
        if self.surface != sailry_protocol::plugin::desktop::Surface::Settings {
            return None;
        }
        let package = self
            .metadata
            .read(cx)
            .entries
            .get(name)?
            .summary
            .reference();
        self.ready_for(&package, cx).then_some(package)
    }

    fn bind(
        source: Option<Entity<Chat>>,
        binding: Binding,
        session: Option<sailry_protocol::SessionId>,
        cx: &mut Context<Self>,
    ) -> Self {
        let metadata = cx.new(|_| Metadata::new(binding.client.clone(), binding.runtime.clone()));
        let observer = cx.observe(&metadata, |_, _, cx| cx.notify());
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let client = binding.client.clone();
        let (sender, mut receiver) = tokio::sync::watch::channel(View::default());
        binding.runtime.spawn(async move {
            let _ = client.watch(sender, cancellation).await;
        });
        let watch = cx.spawn(async move |panel, cx| {
            loop {
                let view = receiver.borrow_and_update().clone();
                if panel
                    .update(cx, |panel, cx| panel.accept(view, cx))
                    .is_err()
                {
                    break;
                }
                if receiver.changed().await.is_err() {
                    break;
                }
            }
        });
        Self {
            activity_sources: Default::default(),
            usage_sources: Default::default(),
            workspace: None,
            resource: None,
            browser: None,
            documents: None,
            pending_document: None,
            artifact: None,
            pane: None,
            pane_events: Default::default(),
            pane_focus: false,
            contributions: None,
            content_sized: false,
            composer: None,
            source,
            surface: Default::default(),
            session,
            binding,
            metadata,
            connected: false,
            source_error: None,
            summaries: Vec::new(),
            snapshot: Default::default(),
            selected: None,
            mounted: None,
            loading: false,
            reload: false,
            error: None,
            viewport: None,
            stop,
            load_stop: CancellationToken::new(),
            _watch: watch,
            _metadata: observer,
            _header: None,
            _workspace: None,
            load: None,
        }
    }

    fn accept(&mut self, view: View, cx: &mut Context<Self>) {
        let cursor = view.snapshot.as_ref().map_or(0, |snapshot| snapshot.cursor);
        self.connected = view.connected;
        self.source_error = view.error;
        if let Some(snapshot) = view.snapshot {
            self.summaries = snapshot.plugins.clone();
            self.metadata
                .update(cx, |metadata, cx| metadata.accept(&self.summaries, cx));
            *self.snapshot.borrow_mut() = Some(snapshot);
        }
        if !self.scope_available() {
            // Retained views must stop before a removed worktree reaches observers.
            self.release();
            self.reload = false;
            self.error = None;
            cx.notify();
            return;
        }
        if let Some(mounted) = &self.mounted {
            mounted.observe(cursor, self.connected);
            mounted
                .host()
                .observe_settings(&self.summaries, self.connected);
        }
        if let Some(package) = &self.selected
            && (!self.connected
                || !self.available(package)
                || self
                    .mounted
                    .as_ref()
                    .is_some_and(|mounted| !mounted.active()))
        {
            self.reload |= self.mounted.is_some() || self.loading;
            if self
                .mounted
                .as_ref()
                .is_some_and(|mounted| mounted.host().retain_settings())
            {
                cx.notify();
                return;
            }
            if !self.connected || self.settings_refresh() {
                if let Some(mounted) = &self.mounted {
                    // Retain presentation only; the obsolete scope must stop immediately.
                    mounted.close();
                }
            } else {
                self.release();
                self.error = Some("plugins_view_unavailable");
            }
        }
        cx.notify();
    }

    fn settings_refresh(&self) -> bool {
        self.connected
            && self.selected.as_ref().is_some_and(|selected| {
                self.summaries.iter().any(|summary| {
                    (summary.enabled
                        || self.surface == sailry_protocol::plugin::desktop::Surface::Settings)
                        && summary.name == selected.name
                        && summary.digest == selected.digest
                        && summary.settings_revision != selected.settings_revision
                })
            })
    }

    fn scope_available(&self) -> bool {
        self.binding.worktree.is_none_or(|id| {
            self.snapshot
                .borrow()
                .as_ref()
                .is_some_and(|snapshot| snapshot.worktrees.iter().any(|tree| tree.id == id))
        })
    }

    pub(super) fn available(&self, package: &Reference) -> bool {
        self.scope_available()
            && self.summaries.iter().any(|summary| {
                (summary.enabled
                    || self.surface == sailry_protocol::plugin::desktop::Surface::Settings)
                    && summary.reference() == *package
            })
    }

    pub(crate) fn enter(
        &mut self,
        package: Reference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.metadata
            .update(cx, |metadata, cx| metadata.refresh(cx));
        if let Some(mounted) = &self.mounted {
            mounted.host().enter();
        } else if !self.loading && self.connected && self.available(&package) {
            self.open(package, window, cx);
        }
    }

    pub(super) fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface == sailry_protocol::plugin::desktop::Surface::Settings
            && self
                .mounted
                .as_ref()
                .is_some_and(|mounted| !mounted.active())
        {
            self.reload = true;
        }
        if let Some(mounted) = &self.mounted
            && mounted.host().retain_settings()
        {
            let package = mounted.host().context().package;
            if self.available(&package) {
                self.selected = Some(package);
                self.reload = false;
            }
            return;
        }
        if !self.connected || !self.reload || self.loading {
            return;
        }
        let Some(selected) = &self.selected else {
            return;
        };
        if let Some(package) = self
            .summaries
            .iter()
            .find(|summary| {
                (summary.enabled
                    || self.surface == sailry_protocol::plugin::desktop::Surface::Settings)
                    && summary.name == selected.name
            })
            .map(Summary::reference)
        {
            // A settings revision or reconnection invalidates the old host scope.
            // Recreate the visible view with current provenance, never old drafts.
            self.open(package, window, cx);
        }
    }

    fn release(&mut self) {
        self.pane_events = Default::default();
        self._header = None;
        self.load_stop.cancel();
        self.load = None;
        self.mounted = None;
        self.loading = false;
    }

    #[cfg(test)]
    pub(super) fn back(&mut self, cx: &mut Context<Self>) {
        self.release();
        self.reload = false;
        self.selected = None;
        self.error = None;
        cx.notify();
    }

    pub(crate) fn open(&mut self, package: Reference, window: &mut Window, cx: &mut Context<Self>) {
        if !self.connected || !self.available(&package) {
            return;
        }
        if self.reload
            && let Some(mounted) = self.mounted.as_ref()
            && self.selected.as_ref().is_some_and(|selected| {
                selected.name == package.name && selected.digest == package.digest
            })
        {
            self.load_stop.cancel();
            self.load = None;
            mounted.close();
        } else {
            self.release();
        }
        self.reload = false;
        self.selected = Some(package.clone());
        self.error = None;
        self.loading = true;
        self.load_stop = self.stop.child_token();
        let cancellation = self.load_stop.clone();
        let binding = self.binding.clone();
        let expected = package.clone();
        let surface = self.surface;
        let job = self.binding.runtime.spawn(async move {
            let result =
                loading::read(&binding.client, expected.clone(), surface, &cancellation).await?;
            let bundle = match result {
                Ok(Output::PluginView(bundle)) => bundle,
                Err(error) => {
                    eprintln!(
                        "sailry-desktop: plugin {} view read failed: {error}",
                        expected.name
                    );
                    return Some(Err("plugins_view_read_failed"));
                }
                Ok(_) => return Some(Err("plugins_view_read_failed")),
            };
            let runtime = binding.runtime.handle().clone();
            let name = expected.name.clone();
            let result = binding
                .runtime
                .spawn_blocking(move || Cache::create(bundle, &expected, runtime))
                .await;
            Some(match result {
                Ok(Ok(cache)) => Ok(cache),
                Ok(Err(error)) => {
                    eprintln!("sailry-desktop: plugin {name} view cache failed: {error}");
                    Err("plugins_view_read_failed")
                }
                Err(error) => {
                    eprintln!("sailry-desktop: plugin {name} view cache task failed: {error}");
                    Err("plugins_view_read_failed")
                }
            })
        });
        let cancellation = self.load_stop.clone();
        self.load = Some(cx.spawn_in(window, async move |panel, cx| {
            let result = job.await;
            let _ = panel.update_in(cx, |panel, window, cx| {
                if cancellation.is_cancelled() {
                    return;
                }
                panel.loading = false;
                match result {
                    Ok(Some(Ok(cache))) if panel.connected && panel.available(&package) => {
                        let read_files = panel
                            .metadata
                            .read(cx)
                            .entries
                            .get(&package.name)
                            .and_then(|info| info.extension.as_ref())
                            .is_some_and(|extension| {
                                extension
                                    .actions
                                    .contains(&sailry_protocol::plugin::Action::ReadFiles)
                            });
                        let host = Host::new(
                            panel.binding.client.clone(),
                            Scope {
                                invocation: None,
                                turn: None,
                                surface: panel.surface,
                                package: package.clone(),
                                worktree: panel.binding.worktree,
                                session: panel.session,
                            },
                            panel.binding.runtime.handle().clone(),
                            read_files,
                            panel.contributions.clone(),
                        );
                        host.observe(
                            panel
                                .snapshot
                                .borrow()
                                .as_ref()
                                .map_or(0, |snapshot| snapshot.cursor),
                            panel.connected,
                        );
                        host.observe_settings(&panel.summaries, panel.connected);
                        let conversation = panel
                            .metadata
                            .read(cx)
                            .entries
                            .get(&package.name)
                            .and_then(|info| info.extension.as_ref())
                            .filter(|extension| {
                                use sailry_protocol::plugin::{Action, desktop::Surface};
                                panel.surface == Surface::Workspace
                                    && [Action::ReadConversation, Action::ControlConversation]
                                        .iter()
                                        .all(|action| extension.actions.contains(action))
                            })
                            .and_then(|extension| {
                                Some(super::conversation::Scope {
                                    owner: cx.weak_entity(),
                                    binding: panel.binding.clone(),
                                    package: package.clone(),
                                    snapshot: panel.snapshot.clone(),
                                    declarations: extension.desktop.as_ref()?.conversations.clone(),
                                    source: panel
                                        .source
                                        .as_ref()
                                        .filter(|_| panel.matches_source(cx))
                                        .cloned(),
                                })
                            });
                        let terminal = panel
                            .metadata
                            .read(cx)
                            .entries
                            .get(&package.name)
                            .and_then(|info| info.extension.as_ref())
                            .filter(|extension| {
                                panel.surface
                                    == sailry_protocol::plugin::desktop::Surface::Workspace
                                    && ((panel.binding.worktree.is_some()
                                        && [
                                            sailry_protocol::plugin::Action::ReadTerminals,
                                            sailry_protocol::plugin::Action::ControlTerminals,
                                        ]
                                        .iter()
                                        .all(|action| extension.actions.contains(action)))
                                        || [
                                            sailry_protocol::plugin::Action::ReadSsh,
                                            sailry_protocol::plugin::Action::ControlSsh,
                                        ]
                                        .iter()
                                        .all(|action| extension.actions.contains(action)))
                            })
                            .map(|extension| super::terminals::Scope {
                                binding: panel.binding.clone(),
                                snapshot: panel.snapshot.clone(),
                                ssh: [
                                    sailry_protocol::plugin::Action::ReadSsh,
                                    sailry_protocol::plugin::Action::ControlSsh,
                                ]
                                .iter()
                                .all(|action| extension.actions.contains(action)),
                                terminals: [
                                    sailry_protocol::plugin::Action::ReadTerminals,
                                    sailry_protocol::plugin::Action::ControlTerminals,
                                ]
                                .iter()
                                .all(|action| extension.actions.contains(action)),
                                context: Scope {
                                    invocation: None,
                                    turn: None,
                                    surface: panel.surface,
                                    package: package.clone(),
                                    worktree: panel.binding.worktree,
                                    session: panel.session,
                                },
                            });
                        panel.pane_events = Default::default();
                        let actions = panel
                            .metadata
                            .read(cx)
                            .entries
                            .get(&package.name)
                            .and_then(|info| info.extension.as_ref())
                            .map(|extension| extension.actions.as_slice())
                            .unwrap_or_default();
                        let browser = super::browser::Scope {
                            browser: panel.browser.clone(),
                            read: actions.contains(&sailry_protocol::plugin::Action::ReadBrowser),
                            control: actions
                                .contains(&sailry_protocol::plugin::Action::ControlBrowser),
                            settings: panel.surface
                                == sailry_protocol::plugin::desktop::Surface::Settings,
                        };
                        match Mounted::new(
                            cache,
                            host.clone(),
                            &package.name,
                            super::runtime::Scopes {
                            owner: cx.weak_entity(),
                            conversation,
                            composer: super::composer::Scope {
                                source: panel.composer.clone(),
                                read: actions
                                    .contains(&sailry_protocol::plugin::Action::ReadConversation),
                                control: actions.contains(
                                    &sailry_protocol::plugin::Action::ControlConversation,
                                ),
                            },
                            location: super::location::Scope {
                                binding: panel.binding.clone(),
                                source: panel.location_source(),
                                snapshot: panel.snapshot.borrow().clone(),
                                connected: panel.connected,
                                read: actions
                                    .contains(&sailry_protocol::plugin::Action::ReadWorktrees),
                                write: actions
                                    .contains(&sailry_protocol::plugin::Action::WriteWorktrees),
                                session_control: actions.contains(
                                    &sailry_protocol::plugin::Action::ControlConversation,
                                ),
                            },
                            terminal,
                            browser,
                            pane: super::panes::Scope {
                                resource: panel.pane,
                                events: panel.pane_events.clone(),
                            },
                            workspace: panel.workspace.clone(),
                            resource: panel.resource.clone(),
                            documents: super::documents::Scope {
                                controller: panel.documents.clone(),
                                read: actions.contains(&sailry_protocol::plugin::Action::ReadFiles),
                                write: actions
                                    .contains(&sailry_protocol::plugin::Action::WriteFiles),
                            },
                            artifact: panel.artifact.clone(),
                            usage_sources: panel.usage_sources.clone(),
                            activity_sources: panel.activity_sources.clone(),
                            file_transfers: super::file_transfers::Scope {
                                binding: panel.binding.clone(),
                                documents: panel.documents.clone(),
                                read: actions.contains(&sailry_protocol::plugin::Action::ReadFiles),
                                write: actions
                                    .contains(&sailry_protocol::plugin::Action::WriteFiles),
                            },
                            },
                            window,
                            cx,
                        ) {
                            Ok(mounted) => {
                                if panel.pane_focus
                                    && let Some(super::panes::Resource::Terminal(id)) = panel.pane
                                {
                                    mounted.terminals.focus(id, window, cx);
                                    panel.pane_focus = false;
                                }
                                panel._header =
                                    Some(cx.observe(&mounted.header, |_, _, cx| cx.notify()));
                                panel.mounted = Some(mounted);
                            }
                            Err(error) => {
                                host.close();
                                eprintln!(
                                    "sailry-desktop: plugin {} view initialization failed: {error:#}",
                                    package.name
                                );
                                panel.error = Some("plugins_view_load_failed");
                            }
                        }
                    }
                    Ok(Some(Err(error))) => panel.error = Some(error),
                    _ => panel.error = Some("plugins_view_unavailable"),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Panel {
    pub(crate) fn launcher_entries(&self, cx: &App) -> Vec<(String, Reference)> {
        self.metadata
            .read(cx)
            .entries
            .values()
            .filter_map(|info| {
                if !info.summary.enabled {
                    return None;
                }
                let extension = info.extension.as_ref()?;
                if extension.scope != sailry_protocol::plugin::Scope::Host {
                    return None;
                }
                let desktop = extension.desktop.as_ref()?;
                if !desktop.renderers.is_empty() {
                    return None;
                }
                let panel = desktop.panel.as_ref()?;
                Some((
                    panel.label(&rust_i18n::locale()).into(),
                    info.summary.reference(),
                ))
            })
            .collect()
    }

    pub(crate) fn renderer_entry(
        &self,
        resource: sailry_protocol::plugin::desktop::ResourceKind,
        cx: &App,
    ) -> Option<(String, Reference)> {
        self.metadata.read(cx).entries.values().find_map(|info| {
            if !info.summary.enabled {
                return None;
            }
            let extension = info.extension.as_ref()?;
            if extension.scope != sailry_protocol::plugin::Scope::Host {
                return None;
            }
            let desktop = extension.desktop.as_ref()?;
            if !desktop
                .renderers
                .iter()
                .any(|renderer| renderer.resource == resource)
            {
                return None;
            }
            Some((
                desktop.panel.as_ref()?.label(&rust_i18n::locale()).into(),
                info.summary.reference(),
            ))
        })
    }
}
