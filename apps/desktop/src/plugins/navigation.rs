//! Manifest-contributed sidebar entries use the same versioned plugin panel host.
use super::{Panel, metadata::Metadata};
use crate::{conversation::live::Binding, preview::Page, shell::Shell};
use gpui_kit::*;
use sailry_client::Client;
use sailry_protocol::{
    NodeId, WorktreeId,
    plugin::{Reference, Scope},
};
use std::sync::Arc;
mod desktop;

/// Navigation is a UI action; the existing conversation route owns all loading.
pub(super) fn ui(
    module: gpui_shell::HostModule,
    owner: WeakEntity<Panel>,
    host: Arc<super::host::Host>,
) -> gpui_shell::HostModule {
    let module =
        super::references::module(super::layout::extend(module), owner.clone(), host.clone());
    let module = super::images::module(module, owner.clone(), host.clone());
    let module = super::documents::routes::module(module, owner.clone(), host.clone());
    let declarations = format!(
        "{}\nexport function openSession(session: string): void;\nexport function openSettings(): void;",
        module.declared().unwrap_or_default()
    );
    let settings_owner = owner.clone();
    let settings_host = host.clone();
    let target_host = host.clone();
    let target_owner = owner.clone();
    module
        .function("selectSettingsHost", move |args| {
            target_host.check()?;
            let node = super::usage::decode_node(args.string(0)?)?;
            gpui_shell::with_current_app(|cx| {
                target_owner.update(cx, |panel, cx| {
                    if panel.surface != sailry_protocol::plugin::desktop::Surface::Settings
                        || !panel.resource_active()
                        || !panel
                            .usage_sources
                            .get()
                            .iter()
                            .any(|source| source.client.target() == node)
                    {
                        return Err(gpui_shell::HostError::new("settings host is unavailable"));
                    }
                    cx.emit(SelectSettingsHost(node));
                    Ok(())
                })
            })
            .ok_or_else(|| gpui_shell::HostError::new("navigation requires an active view"))?
            .map_err(|_| gpui_shell::HostError::new("plugin view is closed"))??;
            Ok(gpui_shell::HostValue::Null)
        })
        .function("openSettings", move |_| {
            settings_host.check()?;
            gpui_shell::with_current_app(|cx| {
                settings_owner
                    .update(cx, |panel, cx| {
                        if panel.resource_active()
                            && let Some(package) = &panel.selected
                        {
                            cx.emit(OpenSettings {
                                node: panel.binding.client.target(),
                                package: package.clone(),
                            });
                        }
                    })
                    .map_err(|_| gpui_shell::HostError::new("plugin view is closed"))
            })
            .ok_or_else(|| gpui_shell::HostError::new("navigation requires an active view"))??;
            Ok(gpui_shell::HostValue::Null)
        })
        .function("openSession", move |args| {
            host.check()?;
            let session = args
                .string(0)?
                .parse::<sailry_protocol::SessionId>()
                .map_err(|_| gpui_shell::HostError::new("invalid session ID"))?;
            gpui_shell::with_current_app(|cx| {
                owner
                    .update(cx, |panel, cx| {
                        if panel.connected && panel.mounted.is_some() {
                            cx.emit(super::panel::OpenSession {
                                node: panel.binding.client.target(),
                                session,
                            });
                        }
                    })
                    .map_err(|_| gpui_shell::HostError::new("plugin view is closed"))
            })
            .ok_or_else(|| gpui_shell::HostError::new("navigation requires an active view"))??;
            Ok(gpui_shell::HostValue::Null)
        })
        .declarations(declarations)
}

pub(crate) struct SelectSettingsHost(pub NodeId);
impl EventEmitter<SelectSettingsHost> for Panel {}

pub(super) struct OpenSettings {
    node: NodeId,
    package: Reference,
}
impl EventEmitter<OpenSettings> for Panel {}

fn panel_sources(sources: &[super::usage::Source], binding: &Binding) -> Vec<super::usage::Source> {
    let mut sources = sources.to_vec();
    if !sources
        .iter()
        .any(|source| source.client.target() == binding.client.target())
    {
        sources.push(super::usage::Source {
            client: binding.client.clone(),
            label: binding.host.to_string(),
        });
    }
    sources
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub node: NodeId,
    pub worktree: Option<WorktreeId>,
    pub scope: Scope,
    pub label: String,
    pub icon: sailry_protocol::plugin::desktop::Icon,
    pub package: Reference,
    pub navigation: sailry_protocol::plugin::desktop::NavigationOptions,
}

impl Entry {
    pub fn selector(&self) -> String {
        format!("navigation-plugin-{:?}-{}", self.node, self.package.name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SettingsEntry {
    pub node: NodeId,
    pub package: Reference,
    pub label: String,
    pub icon: Option<sailry_protocol::plugin::desktop::Icon>,
    pub placement: sailry_protocol::plugin::desktop::SettingsPlacement,
}

pub(crate) struct Navigation {
    node: NodeId,
    pub metadata: Entity<Metadata>,
    desktop: Entity<desktop::Desktop>,
    pub(super) panels: Vec<(Entry, Entity<Panel>)>,
    pub panel: Option<Entity<Panel>>,
    pub selected: Option<Entry>,
    pub label: SharedString,
}

impl Shell {
    pub(crate) fn usage_sources(&self) -> Vec<super::usage::Source> {
        self.live
            .iter()
            .flat_map(|live| {
                live.hosts.keys().filter_map(|node| {
                    live.transport_for(*node)
                        .map(|transport| super::usage::Source {
                            client: Arc::new(Client::new(transport)),
                            label: live.name(*node).to_string(),
                        })
                })
            })
            .collect()
    }

    pub(crate) fn observe_plugin_conversations(
        panel: &Entity<Panel>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            panel,
            window,
            |shell, panel, event: &OpenSettings, window, cx| {
                let state = panel.read(cx);
                if !state.resource_active() || state.selected.as_ref() != Some(&event.package) {
                    return;
                }
                shell.navigate(Page::Settings, window, cx);
                shell.settings_target = Some(event.node);
                shell.settings.update(cx, |settings, cx| {
                    settings.open_plugin_settings(&event.package.name, cx)
                });
            },
        )
        .detach();
        Self::observe_plugin_panes(panel, window, cx);
        Self::observe_plugin_locations(panel, window, cx);
        Self::observe_plugin_references(panel, window, cx);
        Self::observe_plugin_images(panel, window, cx);
        Self::observe_plugin_close(panel, window, cx);
        Self::observe_plugin_activity(panel, window, cx);
        Panel::observe_notifications(panel, window, cx);
        cx.subscribe_in(
            panel,
            window,
            |shell, _, event: &super::panel::ConversationEvent, window, cx| {
                shell.follow_plugin_conversation(
                    event.source.clone(),
                    event.action.clone(),
                    window,
                    cx,
                );
            },
        )
        .detach();
        cx.subscribe_in(
            panel,
            window,
            |shell, _, event: &super::panel::OpenSession, window, cx| {
                shell.open_activity(
                    event.node,
                    sailry_client::activity::Target::Session(event.session),
                    window,
                    cx,
                );
            },
        )
        .detach();
    }

    fn follow_plugin_conversation(
        &mut self,
        source: Entity<crate::conversation::live::View>,
        event: crate::conversation::live::Event,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::{conversation::live::Event, shell::session_scope::Key};
        match event {
            Event::PluginMounted(panel) => {
                Self::observe_plugin_conversations(&panel, window, cx);
                return;
            }
            Event::Settings(node) => {
                self.navigate(Page::Settings, window, cx);
                self.settings_target = Some(node);
                self.settings.update(cx, |settings, cx| {
                    settings.select(crate::settings::Section::Providers, cx)
                });
                return;
            }
            Event::DictationSettings => {
                self.open_dictation_settings(window, cx);
                return;
            }
            _ => {}
        }
        let Some(session) = source.read(cx).summary().cloned() else {
            return;
        };
        let binding = source.read(cx).binding();
        if self.guard_file_navigation(window, cx, {
            let source = source.clone();
            let event = event.clone();
            move |shell, window, cx| {
                shell.follow_plugin_conversation(source.clone(), event.clone(), window, cx)
            }
        }) {
            return;
        }
        let key = (binding.client.target(), session.id);
        let view = match self.chats.views.get(&key) {
            Some(view) => view.clone(),
            None => {
                let view = self.chat_view(binding, Some(session), window, cx);
                self.chats.views.insert(key, view.clone());
                view
            }
        };
        // The existing conversation route supplies all resource actions from this
        // captured Node/session, even when the sidebar selected another host.
        self.activate_session(Key::Session(key.0, key.1), window, cx);
        // A newly mounted route has not observed its catalog yet. Keep the
        // originating view's known renderer availability when forwarding links.
        if matches!(event, Event::File(_) | Event::FileAt(_, _))
            && source.read(cx).renderer_unavailable(
                sailry_protocol::plugin::desktop::ResourceKind::Documents,
                cx,
            )
        {
            return;
        }
        if self.current_chat() == Some(&view) {
            view.update(cx, |_, cx| cx.emit(event));
        }
    }

    pub(crate) fn extension_settings(&self, cx: &App) -> Option<Entry> {
        let state = self.extensions.as_ref()?;
        let entry = state.selected.as_ref()?;
        let info = state
            .panel
            .as_ref()?
            .read(cx)
            .metadata
            .read(cx)
            .entries
            .get(&entry.package.name)?;
        if info.extension.as_ref()?.settings_page.is_none()
            && info
                .settings
                .as_ref()
                .is_none_or(|schema| schema.properties.is_empty())
        {
            return None;
        }
        Some(entry.clone())
    }

    pub(crate) fn configure_extension(
        &mut self,
        entry: Entry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(panel) = self
            .extensions
            .as_ref()
            .and_then(|state| state.panel.as_ref())
        {
            let state = panel.read(cx);
            if state.binding.client.target() != entry.node
                || state
                    .selected
                    .as_ref()
                    .is_none_or(|selected| selected.name != entry.package.name)
            {
                return;
            }
            let info = state.metadata.read(cx).entries.get(&entry.package.name);
            if let Some(info) = info.filter(|info| {
                info.extension
                    .as_ref()
                    .is_some_and(|extension| extension.settings_page.is_none())
            }) {
                let binding = state.binding.clone();
                let expected = info.summary.clone();
                crate::settings::Workspace::configure_panel(binding, expected, window, cx);
                return;
            }
        }
        self.navigate(Page::Settings, window, cx);
        self.settings_target = Some(entry.node);
        self.settings.update(cx, |settings, cx| {
            settings.open_plugin_settings(&entry.package.name, cx)
        });
    }

    pub(crate) fn observe_plugin_errors(window: &Window, cx: &mut Context<Self>) {
        crate::feedback::observe(window, cx, |shell: &Self, cx| {
            shell
                .extensions
                .as_ref()
                .map(|state| {
                    state
                        .metadata
                        .read(cx)
                        .errors
                        .values()
                        .chain(state.desktop.read(cx).metadata.read(cx).errors.values())
                        .copied()
                        .collect()
                })
                .unwrap_or_default()
        });
    }

    pub(crate) fn sync_extensions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(live) = &self.live else {
            return;
        };
        let mut released = false;
        if self
            .extensions
            .as_ref()
            .is_none_or(|state| state.node != live.selected)
        {
            let runtime = cx.global::<crate::backend::Services>().runtime.clone();
            let client = Arc::new(Client::new(live.transport.clone()));
            let metadata = cx.new(|_| Metadata::new(client, runtime));
            cx.observe(&metadata, |_, _, cx| cx.notify()).detach();
            if let Some(state) = &mut self.extensions {
                if state
                    .selected
                    .as_ref()
                    .is_some_and(|entry| entry.scope == Scope::Host)
                {
                    state.panel = None;
                    state.selected = None;
                    released = true;
                }
                state.node = live.selected;
                state.metadata = metadata;
            } else {
                let services = cx.global::<crate::backend::Services>().clone();
                let desktop = cx.new(|cx| desktop::Desktop::new(services, cx));
                cx.observe(&desktop, |_, _, cx| cx.notify()).detach();
                self.extensions = Some(Navigation {
                    node: live.selected,
                    metadata,
                    desktop,
                    panels: Vec::new(),
                    panel: None,
                    selected: None,
                    label: crate::tr("settings_plugins"),
                });
            }
        }
        let state = self.extensions.as_ref().unwrap();
        let summaries = live
            .view
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.plugins.as_slice())
            .unwrap_or(&[]);
        state
            .metadata
            .update(cx, |metadata, cx| metadata.accept(summaries, cx));
        let entries = self.extension_entries(cx);
        let replacement = (self.page == Page::Plugin)
            .then(|| {
                let selected = self.extensions.as_ref()?.selected.as_ref()?;
                entries
                    .iter()
                    .find(|entry| {
                        entry.node == selected.node
                            && entry.scope == selected.scope
                            && entry.package == selected.package
                            && entry.worktree != selected.worktree
                    })
                    .cloned()
            })
            .flatten();
        let sources = self.usage_sources();
        let state = self.extensions.as_mut().unwrap();
        state.panels.retain(|(entry, _)| {
            entries.iter().any(|current| {
                current.node == entry.node
                    && current.scope == entry.scope
                    && current.package == entry.package
            }) && entry.worktree.is_none_or(|tree| {
                live.view.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.node == entry.node
                        && snapshot
                            .worktrees
                            .iter()
                            .any(|worktree| worktree.id == tree)
                })
            })
        });
        // A visible panel can refresh its package/settings revision in place.
        // Retain that replacement too once it belongs to the current catalog.
        if let Some(panel) = &state.panel
            && !state.panels.iter().any(|(_, cached)| cached == panel)
            && let Some(entry) = entries.iter().find(|entry| {
                let panel = panel.read(cx);
                panel.binding.client.target() == entry.node
                    && panel.binding.worktree == entry.worktree
                    && panel.selected.as_ref() == Some(&entry.package)
            })
        {
            state.panels.push((entry.clone(), panel.clone()));
        }
        for (_, panel) in &state.panels {
            let panel = panel.read(cx);
            panel
                .usage_sources
                .set(panel_sources(&sources, &panel.binding));
        }
        if released && self.page == Page::Plugin {
            self.navigate(Page::Host, window, cx);
        } else if let Some(entry) = replacement {
            self.open_extension(entry, window, cx);
        }
    }

    pub(crate) fn command_navigation_available(&self, entry: &Entry, cx: &App) -> bool {
        if !self.extension_entries(cx).contains(entry) {
            return false;
        }
        let Some(state) = &self.extensions else {
            return false;
        };
        let desktop = state.desktop.read(cx);
        let snapshot = self
            .live
            .as_ref()
            .filter(|live| live.view.connected && live.selected == entry.node)
            .and_then(|live| live.view.snapshot.as_ref())
            .filter(|snapshot| snapshot.node == entry.node);
        let connected = match entry.scope {
            Scope::Host => {
                state.node == entry.node
                    && snapshot.is_some_and(|snapshot| {
                        snapshot
                            .plugins
                            .iter()
                            .any(|summary| summary.enabled && summary.reference() == entry.package)
                    })
            }
            Scope::Desktop => desktop.connected && desktop.client.target() == entry.node,
        };
        connected
            && (entry.navigation.target
                != sailry_protocol::plugin::desktop::NavigationTarget::Worktree
                || entry.worktree.is_some())
            && entry.worktree.is_none_or(|id| {
                snapshot.is_some_and(|snapshot| snapshot.worktrees.iter().any(|tree| tree.id == id))
            })
    }

    pub(crate) fn command_settings_entries(&self, cx: &App) -> Vec<SettingsEntry> {
        let Some(state) = &self.extensions else {
            return Vec::new();
        };
        let desktop = state.desktop.read(cx);
        let snapshot = self
            .live
            .as_ref()
            .filter(|live| live.selected == state.node && live.view.connected)
            .and_then(|live| live.view.snapshot.as_ref())
            .filter(|snapshot| snapshot.node == state.node);
        [
            (state.node, Scope::Host, &state.metadata, snapshot.is_some()),
            (
                desktop.client.target(),
                Scope::Desktop,
                &desktop.metadata,
                desktop.connected,
            ),
        ]
        .into_iter()
        .filter(|(_, _, _, connected)| *connected)
        .flat_map(|(node, scope, metadata, _)| {
            metadata.read(cx).entries.values().filter_map(move |info| {
                let extension = info.extension.as_ref()?;
                let page = extension.settings_page.as_ref()?;
                if extension.scope != scope
                    || scope == Scope::Host
                        && !snapshot.is_some_and(|snapshot| {
                            snapshot
                                .plugins
                                .iter()
                                .any(|summary| summary.reference() == info.summary.reference())
                        })
                {
                    return None;
                }
                Some(SettingsEntry {
                    node,
                    package: info.summary.reference(),
                    label: page.navigation.label(&rust_i18n::locale()).into(),
                    icon: page.navigation.icon.clone(),
                    placement: page.placement,
                })
            })
        })
        .collect()
    }

    pub(crate) fn extension_entries(&self, cx: &App) -> Vec<Entry> {
        let Some(state) = &self.extensions else {
            return vec![];
        };
        let desktop = state.desktop.read(cx);
        let tree = self
            .live
            .as_ref()
            .and_then(|live| live.selected_worktree())
            .map(|tree| tree.id);
        [
            (state.node, Scope::Host, &state.metadata),
            (desktop.client.target(), Scope::Desktop, &desktop.metadata),
        ]
        .into_iter()
        .flat_map(|(node, scope, metadata)| {
            metadata.read(cx).entries.values().filter_map(move |info| {
                let extension = info.extension.as_ref()?;
                let desktop = extension.desktop.as_ref()?;
                if !info.summary.enabled || extension.scope != scope {
                    return None;
                }
                let navigation = desktop.navigation.as_ref()?;
                Some(Entry {
                    node,
                    worktree: (desktop.navigation_options.target
                        == sailry_protocol::plugin::desktop::NavigationTarget::Worktree)
                        .then_some(tree)
                        .flatten(),
                    scope,
                    label: navigation.label(&rust_i18n::locale()).to_owned(),
                    icon: navigation.icon.clone()?,
                    package: info.summary.reference(),
                    navigation: desktop.navigation_options,
                })
            })
        })
        .collect()
    }

    pub(crate) fn renderer_navigation(
        &self,
        resource: sailry_protocol::plugin::desktop::ResourceKind,
        cx: &App,
    ) -> Option<Entry> {
        let state = self.extensions.as_ref()?;
        self.extension_entries(cx).into_iter().find(|entry| {
            entry.scope == Scope::Host
                && state.metadata.read(cx).entries.values().any(|info| {
                    info.summary.reference() == entry.package
                        && info
                            .extension
                            .as_ref()
                            .and_then(|extension| extension.desktop.as_ref())
                            .is_some_and(|desktop| {
                                desktop
                                    .renderers
                                    .iter()
                                    .any(|renderer| renderer.resource == resource)
                            })
                })
        })
    }

    pub(crate) fn contribution_declared(
        &self,
        intent: sailry_protocol::plugin::ui::Intent,
        cx: &App,
    ) -> bool {
        self.extensions.as_ref().is_some_and(|state| {
            state.metadata.read(cx).entries.values().any(|info| {
                info.summary.enabled
                    && info.extension.as_ref().is_some_and(|extension| {
                        extension
                            .ui
                            .iter()
                            .any(|entry| entry.intent.as_deref() == Some(intent.as_ref()))
                    })
            })
        })
    }

    pub(crate) fn open_extension(
        &mut self,
        entry: Entry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.live else {
            return;
        };
        if !self.extension_entries(cx).contains(&entry) {
            return;
        }
        if entry.navigation.surface == sailry_protocol::plugin::desktop::Surface::Settings {
            self.navigate(Page::Settings, window, cx);
            self.settings_target = Some(entry.node);
            self.settings.update(cx, |settings, cx| {
                settings.open_plugin_settings(&entry.package.name, cx)
            });
            return;
        }
        let state = self.extensions.as_ref().unwrap();
        let desktop = state.desktop.read(cx);
        let client = if entry.node == desktop.client.target() {
            if !desktop.connected {
                return;
            }
            desktop.client.clone()
        } else {
            if !live.view.connected || entry.node != live.selected {
                return;
            }
            Arc::new(Client::new(live.transport.clone()))
        };
        let host = live.name(entry.node);
        let tree = entry
            .worktree
            .and_then(|id| {
                live.view
                    .snapshot
                    .as_ref()?
                    .worktrees
                    .iter()
                    .find(|tree| tree.id == id)
            })
            .cloned();
        let project = tree
            .as_ref()
            .and_then(|tree| tree.project)
            .and_then(|id| {
                live.view
                    .snapshot
                    .as_ref()?
                    .projects
                    .iter()
                    .find(|project| project.id == id)
            })
            .cloned();
        let sources = self.usage_sources();
        if self.guard_file_navigation(window, cx, {
            let entry = entry.clone();
            move |shell, window, cx| shell.open_extension(entry.clone(), window, cx)
        }) {
            return;
        }
        self.navigate(Page::Plugin, window, cx);
        let state = self.extensions.as_mut().unwrap();
        let loadable = entry.navigation.target
            == sailry_protocol::plugin::desktop::NavigationTarget::Node
            || entry.worktree.is_some();
        if let Some((_, panel)) = state.panels.iter().find(|(cached, _)| {
            cached.node == entry.node
                && cached.scope == entry.scope
                && cached.package == entry.package
                && cached.worktree == entry.worktree
        }) {
            state.panel = Some(panel.clone());
            state.selected = Some(entry.clone());
            if loadable {
                panel.update(cx, |panel, cx| {
                    panel.enter(entry.package.clone(), window, cx)
                });
            }
        }
        if state.selected.as_ref() != Some(&entry) || state.panel.is_none() {
            let runtime = cx.global::<crate::backend::Services>().runtime.clone();
            let binding = Binding {
                client: client.clone(),
                defaults: client,
                runtime,
                project: tree.as_ref().and_then(|tree| tree.project),
                worktree: entry.worktree,
                host,
                project_name: project
                    .map(|project| project.name.into())
                    .unwrap_or_default(),
                branch: tree.map(|tree| tree.path.into()).unwrap_or_default(),
            };
            let documents = self.documents.get(binding.clone(), None, window, cx);
            let sources = panel_sources(&sources, &binding);
            let panel = cx.new(|cx| {
                let panel = Panel::workspace(binding, entry.navigation.details, cx)
                    .with_documents(documents);
                panel.usage_sources.set(sources);
                panel
            });
            Self::observe_plugin_conversations(&panel, window, cx);
            // Wait for the normal Node inventory subscription before loading the
            // selected immutable package. No hard-coded bundled game route.
            let expected = entry.package.clone();
            cx.observe_in(&panel, window, move |_, panel, window, cx| {
                let state = panel.read(cx);
                if loadable
                    && state.connected
                    && state.selected.is_none()
                    && state.available(&expected)
                    && state.metadata.read(cx).settled()
                {
                    panel.update(cx, |panel, cx| panel.open(expected.clone(), window, cx));
                }
                if panel.read(cx).resource_active() {
                    let target = panel.update(cx, |panel, _| panel.pending_document.take());
                    if let Some((path, line)) = target {
                        Self::reveal_document(&panel, path, line, window, cx);
                    }
                }
                cx.notify();
            })
            .detach();
            state.panels.push((entry.clone(), panel.clone()));
            state.panel = Some(panel);
            state.selected = Some(entry.clone());
        }
        state.label = entry.label.into();
        cx.notify();
    }
}
