mod lifecycle;
use super::{host::Host, metadata::Metadata, runtime::Mounted};
use crate::conversation::live::{Binding, View as Chat};
use gpui_kit::*;
use lifecycle::Initialization;
use sailry_client::View;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::{Context as Scope, Reference, Summary};
use std::{cell::RefCell, rc::Rc};

pub(super) type Snapshot = Rc<RefCell<Option<sailry_protocol::Snapshot>>>;

pub(crate) struct Panel {
    pub(crate) activity_sources: super::host::activity::Sources,
    pub(crate) usage_sources: super::usage::Sources,
    pub(crate) workspace: Option<Entity<super::workspace::State>>,
    pub(crate) resource: Option<Entity<super::resource::State>>,
    browser: Option<Entity<crate::browser::Browser>>,
    pub(crate) documents: Option<Entity<super::documents::Controller>>,
    pub(crate) pending_document: Option<super::documents::routes::OpenTarget>,
    pub(crate) artifact: Option<super::previews::Resource>,
    pub(super) document: Option<super::artifact::Resource>,
    pub(super) pane: Option<super::panes::Resource>,
    pub(super) pane_events: super::panes::Events,
    pub(super) pane_focus: bool,
    entry_pending: Option<Reference>,
    composer: Option<WeakEntity<Chat>>,
    pub(super) providers: Option<Entity<super::providers::Source>>,
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
    package_cursors: std::collections::BTreeMap<String, u64>,
    pub(super) snapshot: Snapshot,
    pub(super) selected: Option<Reference>,
    pub(super) target: Option<String>,
    pub(super) mounted: Option<Mounted>,
    pub(super) initialization: Initialization,
    failures: lifecycle::Failures,
    prepared: Option<super::preload::Prepared>,
    pub(super) error: Option<&'static str>,
    pub(super) viewport: Option<Size<Pixels>>,
    pub(super) stop: CancellationToken,
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

/// An embedded existing session was shown, which reads it like opening it.
pub(crate) struct ReadSession {
    pub node: sailry_protocol::NodeId,
    pub session: sailry_protocol::SessionId,
}
impl EventEmitter<ReadSession> for Panel {}

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
            && !self.initialization.loading()
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

    pub(crate) fn settings(binding: Binding, content_sized: bool, cx: &mut Context<Self>) -> Self {
        let mut panel = Self::standalone(binding, cx);
        panel.surface = sailry_protocol::plugin::desktop::Surface::Settings;
        panel.content_sized = content_sized;
        panel
    }

    pub(crate) fn with_prepared(mut self, prepared: Option<super::preload::Prepared>) -> Self {
        self.prepared = prepared;
        self
    }

    pub(crate) fn with_target(mut self, name: impl Into<String>) -> Self {
        self.target = Some(name.into());
        self
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

    pub(super) fn share_metadata(&mut self, metadata: Entity<Metadata>, cx: &mut Context<Self>) {
        self._metadata = cx.observe(&metadata, |_, _, cx| cx.notify());
        self.metadata = metadata;
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
            && self.initialization == Initialization::Unloaded
            && self.available(package)
            && self.metadata.read(cx).ready(package)
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
            document: None,
            pane: None,
            pane_events: Default::default(),
            pane_focus: false,
            entry_pending: None,
            contributions: None,
            content_sized: false,
            composer: None,
            providers: None,
            source,
            surface: Default::default(),
            session,
            binding,
            metadata,
            connected: false,
            source_error: None,
            summaries: Vec::new(),
            package_cursors: Default::default(),
            snapshot: Default::default(),
            selected: None,
            target: None,
            mounted: None,
            initialization: Initialization::Unloaded,
            failures: Default::default(),
            prepared: None,
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
        if self.stop.is_cancelled() {
            return;
        }
        let cursor = view.snapshot.as_ref().map_or(0, |snapshot| snapshot.cursor);
        self.package_cursors = view.package_cursors;
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
            if self.snapshot.borrow().is_some() {
                self.close(cx);
            } else if self.selected.is_some()
                || self.initialization.loading()
                || self.mounted.is_some()
            {
                self.release();
            }
            self.initialization = Initialization::Unloaded;
            self.error = None;
            cx.notify();
            return;
        }
        if let Some(mounted) = &self.mounted {
            mounted.observe(cursor, &self.package_cursors, self.connected);
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
            if self.mounted.is_some() || self.initialization.loading() {
                self.initialization = Initialization::Invalidated;
            }
            if self
                .mounted
                .as_ref()
                .is_some_and(|mounted| mounted.host().retain_settings())
            {
                cx.notify();
                return;
            }
            if !self.connected {
                self.load_stop.cancel();
                self.load = None;
                if let Some(mounted) = &self.mounted {
                    // Retain presentation only; the obsolete scope must stop immediately.
                    mounted.close();
                }
            } else if self.reference_changed()
                || self.initialization == Initialization::Invalidated
                    && self.available(package)
                    && self
                        .mounted
                        .as_ref()
                        .is_some_and(|mounted| !mounted.active())
            {
                self.initialization = Initialization::Invalidated;
                if let Some(mounted) = &self.mounted {
                    mounted.close();
                }
                // A committed replacement is not an unavailable extension. A same-
                // digest settings refresh keeps only its closed presentation until
                // replacement; changed package content cannot retain the old view.
                let content_changed = self.selected.as_ref().is_some_and(|selected| {
                    self.summaries.iter().any(|summary| {
                        summary.name == selected.name && summary.digest != selected.digest
                    })
                });
                if content_changed {
                    self.release();
                    self.initialization = Initialization::Invalidated;
                } else {
                    self.load_stop.cancel();
                    self.load = None;
                    self.initialization = Initialization::Invalidated;
                }
                self.error = None;
            } else {
                let uninstalled = self.document.is_some()
                    && !self
                        .summaries
                        .iter()
                        .any(|summary| summary.name == package.name);
                self.release();
                self.error = Some(if uninstalled {
                    "plugins_uninstalled"
                } else {
                    "plugins_view_unavailable"
                });
            }
        }
        cx.notify();
    }

    fn reference_changed(&self) -> bool {
        self.connected
            && self.selected.as_ref().is_some_and(|selected| {
                self.summaries.iter().any(|summary| {
                    (summary.enabled
                        || self.surface == sailry_protocol::plugin::desktop::Surface::Settings)
                        && summary.name == selected.name
                        && summary.reference() != *selected
                })
            })
    }

    fn scope_available(&self) -> bool {
        if self.binding.project.is_some_and(|project| {
            self.snapshot
                .borrow()
                .as_ref()
                .is_some_and(|snapshot| !snapshot.projects.iter().any(|entry| entry.id == project))
        }) {
            return false;
        }
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
        if self.stop.is_cancelled() {
            return;
        }
        self.metadata
            .update(cx, |metadata, cx| metadata.retry(Some(&package.name), cx));
        if (self
            .mounted
            .as_ref()
            .is_none_or(|mounted| !mounted.active())
            || self.selected.as_ref() != Some(&package))
            && (!self.initialization.loading() || self.selected.as_ref() != Some(&package))
            && self.connected
            && self.available(&package)
            && self.metadata.read(cx).ready(&package)
        {
            self.open(package.clone(), window, cx);
        }
        if self.selected.as_ref() == Some(&package)
            || (self.selected.is_none() && self.target.as_deref() == Some(package.name.as_str()))
        {
            self.request_entry(package);
        }
    }

    fn request_entry(&mut self, package: Reference) {
        if self.stop.is_cancelled() {
            return;
        }
        if let Some(mounted) = &self.mounted
            && mounted.active()
            && self.selected.as_ref() == Some(&package)
            && self.available(&package)
        {
            mounted.host().enter();
            self.entry_pending = None;
        } else {
            self.entry_pending = Some(package);
        }
    }

    pub(super) fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface == sailry_protocol::plugin::desktop::Surface::Settings
            && self.initialization == Initialization::Published
            && self
                .mounted
                .as_ref()
                .is_some_and(|mounted| !mounted.active())
        {
            self.initialization = Initialization::Invalidated;
        }
        if let Some(mounted) = &self.mounted
            && mounted.host().retain_settings()
        {
            let package = mounted.host().context().package;
            if self.available(&package) {
                self.selected = Some(package);
                self.initialization = Initialization::Published;
                self.failures.clear();
            }
            return;
        }
        if !self.connected || self.initialization != Initialization::Invalidated {
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
        self.entry_pending = None;
        self.pane_events = Default::default();
        self._header = None;
        self.load_stop.cancel();
        self.load = None;
        self.mounted = None;
        self.initialization = Initialization::Unloaded;
    }

    fn report_failure(&mut self, code: sailry_protocol::ErrorCode, stage: &'static str) {
        use sailry_protocol::notification::{DiagnosticPlugin, SystemDiagnostic, SystemSource};
        let Some(package) = &self.selected else {
            return;
        };
        let Some(summary) = self
            .summaries
            .iter()
            .find(|summary| summary.reference() == *package)
        else {
            return;
        };
        if !self.failures.record(package, code, stage) {
            return;
        }
        let mut report = SystemDiagnostic::new(
            sailry_protocol::RequestId::new(),
            SystemSource::PluginInitialization,
            code,
        );
        if let Some(version) = &summary.version {
            report.plugin = Some(DiagnosticPlugin {
                name: package.name.clone(),
                version: version.clone(),
            });
        }
        report.worktree = self.binding.worktree;
        report.session = self.session;
        report.operation = Some(stage.into());
        let mut entry = sailry_node_runtime::logging::Entry::new(
            sailry_node_runtime::logging::Area::Plugin,
            sailry_node_runtime::logging::Phase::Failed,
        );
        entry.context.node = Some(self.binding.client.target());
        entry.context.package = Some(package.clone());
        entry.context.request = Some(report.incident);
        entry.context.worktree = self.binding.worktree;
        entry.context.session = self.session;
        entry.reason = Some(stage);
        entry.code = Some(code);
        entry.emit();
        let client = self.binding.client.clone();
        self.binding.runtime.spawn(async move {
            let _ = client.report_diagnostic(report).await;
        });
    }

    pub(super) fn disconnect(&mut self, cx: &mut Context<Self>) {
        if !self.connected && !self.initialization.loading() {
            return;
        }
        self.connected = false;
        self.load_stop.cancel();
        self.load = None;
        if self.mounted.is_some() || self.initialization.loading() {
            self.initialization = Initialization::Invalidated;
        }
        if let Some(mounted) = &self.mounted {
            mounted.close();
        }
        cx.notify();
    }

    pub(crate) fn close(&mut self, cx: &mut Context<Self>) {
        self.stop.cancel();
        if self.contributions.is_none() {
            self.metadata.update(cx, |metadata, cx| metadata.close(cx));
        }
        self.release();
        self.prepared = None;
        self.selected = None;
        self.failures.clear();
        self.target = None;
        self.initialization = Initialization::Unloaded;
        self.connected = false;
        self.source_error = None;
        self.error = None;
        cx.notify();
    }

    pub(crate) fn open(&mut self, package: Reference, window: &mut Window, cx: &mut Context<Self>) {
        if !self.connected || !self.available(&package) || !self.metadata.read(cx).ready(&package) {
            return;
        }
        let entry_pending = self
            .entry_pending
            .take()
            .filter(|pending| pending == &package);
        if self.initialization == Initialization::Invalidated
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
        self.entry_pending = entry_pending;
        self.initialization = Initialization::Unloaded;
        self.selected = Some(package.clone());
        self.target = Some(package.name.clone());
        self.error = None;
        self.initialization = Initialization::Loading;
        if let Some(bridge) = &self.contributions {
            bridge.restart();
        }
        self.load_stop = self.stop.child_token();
        let cancellation = self.load_stop.clone();
        let binding = self.binding.clone();
        let expected = package.clone();
        let surface = self.surface;
        let prepared = self
            .prepared
            .take()
            .filter(|prepared| prepared.package == expected);
        let job = self.binding.runtime.spawn(async move {
            if let Some(prepared) = prepared {
                let result = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return None,
                    result = prepared.load() => result,
                };
                if result.is_some() {
                    return result;
                }
            }
            super::cache::prepare(
                binding.client,
                binding.runtime.handle().clone(),
                expected,
                surface,
                cancellation,
            )
            .await
        });
        let cancellation = self.load_stop.clone();
        self.load = Some(cx.spawn_in(window, async move |panel, cx| {
            let result = job.await;
            let _ = panel.update_in(cx, |panel, window, cx| {
                if cancellation.is_cancelled() {
                    return;
                }
                panel.initialization = Initialization::Failed;
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
                        let install_models = panel
                            .metadata
                            .read(cx)
                            .entries
                            .get(&package.name)
                            .filter(|info| info.summary.reference() == package)
                            .and_then(|info| info.extension.as_ref())
                            .is_some_and(|extension| {
                                extension
                                    .actions
                                    .contains(&sailry_protocol::plugin::Action::InstallModels)
                            });
                        let manager = install_models
                            .then(|| crate::dictation::Service::models(cx).ok())
                            .flatten();
                        host.model_services(manager, panel.binding.runtime.clone(), install_models);
                        host.observe(
                            panel
                                .snapshot
                                .borrow()
                                .as_ref()
                                .map_or(0, |snapshot| snapshot.cursor),
                            panel
                                .package_cursors
                                .get(&package.name)
                                .copied()
                                .unwrap_or(0),
                            panel.connected,
                        );
                        host.observe_settings(&panel.summaries, panel.connected);
                        if panel.entry_pending.take().as_ref() == Some(&package) {
                            host.enter();
                        }
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
                        let model_resources = panel
                            .metadata
                            .read(cx)
                            .entries
                            .get(&package.name)
                            .filter(|info| info.summary.reference() == package)
                            .and_then(|info| info.extension.as_ref())
                            .is_some_and(|extension| !extension.model_resources.is_empty());
                        match Mounted::new(
                            cache,
                            host.clone(),
                            &package.name,
                            super::runtime::Scopes {
                                owner: cx.weak_entity(),
                                conversation,
                                providers: panel.providers.clone(),
                                composer: super::composer::Scope {
                                    source: panel.composer.clone(),
                                    read: actions.contains(
                                        &sailry_protocol::plugin::Action::ReadConversation,
                                    ),
                                    model: actions
                                        .contains(&sailry_protocol::plugin::Action::ProvideModels)
                                        || model_resources,
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
                                    read: actions
                                        .contains(&sailry_protocol::plugin::Action::ReadFiles),
                                    write: actions
                                        .contains(&sailry_protocol::plugin::Action::WriteFiles),
                                },
                                artifact: panel.artifact.clone(),
                                document: panel.document.clone(),
                                usage_sources: panel.usage_sources.clone(),
                                activity_sources: panel.activity_sources.clone(),
                                file_transfers: super::file_transfers::Scope {
                                    binding: panel.binding.clone(),
                                    documents: panel.documents.clone(),
                                    read: actions
                                        .contains(&sailry_protocol::plugin::Action::ReadFiles),
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
                                panel.initialization = Initialization::Published;
                                panel.failures.clear();
                            }
                            Err(_) => {
                                host.close();
                                panel.error = Some("plugins_view_load_failed");
                                panel.report_failure(
                                    sailry_protocol::ErrorCode::Internal,
                                    "view_initialize",
                                );
                            }
                        }
                    }
                    Ok(Some(Err(error))) => {
                        panel.error = Some(error.key);
                        panel.report_failure(error.code, error.stage);
                    }
                    _ => panel.error = Some("plugins_view_unavailable"),
                }
                if panel.initialization.failed() {
                    panel.mounted = None;
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
