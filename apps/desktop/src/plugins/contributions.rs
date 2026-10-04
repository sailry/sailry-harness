//! Shared state and events for a plugin's declared controller slots.
pub(crate) mod bridge;
mod controls;
pub(crate) mod intents;
pub(crate) mod native;
mod picker;
pub(crate) use controls::Form;

use crate::{
    conversation::live::Binding,
    plugins::{Panel, metadata::Metadata},
};
use bridge::Bridge;
use gpui_kit::*;
use sailry_protocol::{
    NodeId, Session, Snapshot, WorktreeId,
    plugin::{
        Reference, Scope,
        desktop::Surface,
        ui::{self, Contribution, Slot, State},
    },
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Key {
    pub node: NodeId,
    pub worktree: Option<WorktreeId>,
    pub session: Option<sailry_protocol::SessionId>,
    pub package: Reference,
    pub id: String,
}

#[derive(Clone)]
pub(crate) struct Entry {
    pub key: Key,
    pub declaration: Contribution,
    pub state: State,
}

struct Mounted {
    overlay: bool,
    package: Reference,
    bridge: Arc<Bridge>,
    states: BTreeMap<String, State>,
    panel: Entity<Panel>,
    _observer: Subscription,
    _updates: Task<()>,
}

impl Mounted {
    fn active(&self, cx: &App) -> bool {
        let panel = self.panel.read(cx);
        panel.connected
            && !panel.loading
            && panel.selected.as_ref() == Some(&self.package)
            && panel.available(&self.package)
            && panel
                .mounted
                .as_ref()
                .is_some_and(|mounted| mounted.active())
    }
}

pub(crate) enum Event {
    Mounted(Entity<Panel>),
    Action(Box<native::Dispatch>),
}

pub(crate) struct ViewState<'a> {
    pub session: Option<&'a Session>,
    pub assistant: Option<&'a sailry_protocol::plugin::conversation::Binding>,
    pub snapshot: Option<&'a Snapshot>,
    pub connected: bool,
}

pub(crate) struct Registry {
    composer: Option<WeakEntity<crate::conversation::live::View>>,
    surface: Surface,
    node: NodeId,
    worktree: Option<WorktreeId>,
    session: Option<sailry_protocol::SessionId>,
    metadata: Entity<Metadata>,
    catalog_received: bool,
    _metadata: Subscription,
    mounted: BTreeMap<String, Mounted>,
}

impl EventEmitter<Event> for Registry {}

impl Registry {
    #[cfg(test)]
    pub(crate) fn test_panel(&self, name: &str) -> Option<Entity<Panel>> {
        self.mounted.get(name).map(|mounted| mounted.panel.clone())
    }
    pub fn new(
        binding: &Binding,
        source: WeakEntity<crate::conversation::live::View>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut registry = Self::bind(binding, Surface::Composer, cx);
        registry.composer = Some(source);
        registry
    }

    pub fn project(binding: &Binding, cx: &mut Context<Self>) -> Self {
        Self::bind(binding, Surface::Project, cx)
    }

    fn bind(binding: &Binding, surface: Surface, cx: &mut Context<Self>) -> Self {
        let metadata = cx.new(|_| Metadata::new(binding.client.clone(), binding.runtime.clone()));
        let observe = cx.observe(&metadata, |_, _, cx| cx.notify());
        Self {
            composer: None,
            surface,
            node: binding.client.target(),
            worktree: binding.worktree,
            session: None,
            metadata,
            catalog_received: false,
            _metadata: observe,
            mounted: BTreeMap::new(),
        }
    }

    fn accepts(&self, slot: Slot) -> bool {
        slot.is_project() == (self.surface == Surface::Project)
    }

    pub(crate) fn renderer_available(
        &self,
        resource: sailry_protocol::plugin::desktop::ResourceKind,
        cx: &App,
    ) -> bool {
        self.metadata.read(cx).entries.values().any(|info| {
            info.summary.enabled
                && info.extension.as_ref().is_some_and(|extension| {
                    extension.scope == Scope::Host
                        && extension.desktop.as_ref().is_some_and(|desktop| {
                            desktop.panel.is_some()
                                && desktop
                                    .renderers
                                    .iter()
                                    .any(|renderer| renderer.resource == resource)
                        })
                })
        })
    }

    pub(crate) fn renderer_shortcuts(
        &self,
        cx: &App,
    ) -> Vec<(sailry_protocol::plugin::desktop::ResourceKind, String)> {
        let mut shortcuts = Vec::new();
        let mut selected = Vec::new();
        for info in self
            .metadata
            .read(cx)
            .entries
            .values()
            .filter(|info| info.summary.enabled)
        {
            let Some(extension) = info
                .extension
                .as_ref()
                .filter(|extension| extension.scope == Scope::Host)
            else {
                continue;
            };
            let Some(desktop) = extension
                .desktop
                .as_ref()
                .filter(|desktop| desktop.panel.is_some())
            else {
                continue;
            };
            for renderer in &desktop.renderers {
                // Match the launcher's first enabled renderer even when that
                // provider deliberately contributes no shortcut.
                if selected.contains(&renderer.resource) {
                    continue;
                }
                selected.push(renderer.resource);
                if let Some(key) = &renderer.shortcut {
                    shortcuts.push((renderer.resource, key.clone()));
                }
            }
        }
        shortcuts
    }

    pub(crate) fn renderer_unavailable(
        &self,
        resource: sailry_protocol::plugin::desktop::ResourceKind,
        cx: &App,
    ) -> bool {
        self.catalog_received && self.settled(cx) && !self.renderer_available(resource, cx)
    }

    pub fn settled(&self, cx: &App) -> bool {
        self.metadata.read(cx).settled()
    }

    pub fn ready(&self, cx: &App) -> bool {
        self.settled(cx)
            && (self.surface != Surface::Project
                || self
                    .metadata
                    .read(cx)
                    .entries
                    .values()
                    .filter(|info| {
                        info.summary.enabled
                            && info.extension.as_ref().is_some_and(|extension| {
                                extension.scope == Scope::Host
                                    && extension.ui.iter().any(|entry| self.accepts(entry.slot))
                            })
                    })
                    .all(|info| {
                        self.mounted
                            .get(&info.summary.name)
                            .is_some_and(|mounted| mounted.package == info.summary.reference())
                    }))
            && self
                .mounted
                .values()
                .all(|mounted| mounted.active(cx) && mounted.bridge.ready())
    }

    pub fn creation(
        &self,
        resource: sailry_protocol::plugin::desktop::ResourceKind,
        cx: &App,
    ) -> Option<Entry> {
        let entries = self.entries(Slot::Project, cx);
        self.metadata.read(cx).entries.values().find_map(|info| {
            let renderer = info
                .extension
                .as_ref()?
                .desktop
                .as_ref()?
                .renderers
                .iter()
                .find(|renderer| renderer.resource == resource)?;
            entries
                .iter()
                .find(|entry| {
                    entry.key.package == info.summary.reference()
                        && Some(entry.key.id.as_str()) == renderer.create.as_deref()
                })
                .cloned()
        })
    }

    pub(crate) fn intent(&self, intent: impl AsRef<str>, cx: &App) -> Option<Entry> {
        let intent = intent.as_ref();
        self.mounted
            .values()
            .filter(|mounted| mounted.active(cx))
            .find_map(|mounted| {
                let declaration = mounted
                    .bridge
                    .declarations
                    .iter()
                    .filter(|entry| entry.intent.as_deref() == Some(intent))
                    .min_by_key(|entry| !self.accepts(entry.slot))?;
                let state = mounted
                    .bridge
                    .state(&declaration.id)
                    .filter(|state| state.enabled)?;
                Some(Entry {
                    key: Key {
                        node: self.node,
                        worktree: self.worktree,
                        session: self.session,
                        package: mounted.package.clone(),
                        id: declaration.id.clone(),
                    },
                    declaration: declaration.clone(),
                    state,
                })
            })
    }

    pub(crate) fn invoke_intent(
        &mut self,
        intent: impl AsRef<str>,
        value: serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let entry = self
            .intent(intent, cx)
            .ok_or_else(|| "contribution is unavailable".to_owned())?;
        let result = self
            .current(&entry.key)
            .ok_or_else(|| "contribution context changed".to_owned())?
            .bridge
            .invoke_intent(&entry.key.id, value)
            .map_err(str::to_owned);
        cx.notify();
        result
    }

    /// Materialize only explicitly declared roots from this visible captured owner.
    /// The existing controller remains the single owner of forms and pending work.
    pub(crate) fn overlays(&self, window: &Window, cx: &App) -> Vec<AnyElement> {
        self.mounted
            .values()
            .filter(|mounted| mounted.overlay && mounted.active(cx))
            .map(|mounted| {
                deferred(
                    anchored().position(point(px(0.), px(0.))).child(
                        div()
                            .w(window.viewport_size().width)
                            .h(window.viewport_size().height)
                            .child(mounted.panel.clone()),
                    ),
                )
                .with_priority(gpui_kit::base::POPUP_PRIORITY)
                .into_any_element()
            })
            .collect()
    }

    pub fn create(
        &mut self,
        resource: sailry_protocol::plugin::desktop::ResourceKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(entry) = self.creation(resource, cx) {
            self.invoke(
                &entry.key,
                ui::EventKind::Invoke,
                serde_json::Value::Null,
                window,
                cx,
            );
        }
    }

    pub fn sync(
        &mut self,
        binding: &Binding,
        view: ViewState<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ViewState {
            session,
            assistant,
            snapshot,
            connected,
        } = view;
        let session = session.filter(|_| self.surface != Surface::Project);
        let assistant = if self.surface == Surface::Project {
            None
        } else {
            session.map_or(assistant, |session| session.config.assistant.as_ref())
        };
        let id = session.map(|session| session.id);
        if self.node != binding.client.target()
            || self.worktree != binding.worktree
            || self.session != id
        {
            let composer = self.composer.clone();
            *self = Self::bind(binding, self.surface, cx);
            self.composer = composer;
            self.session = id;
        }
        let Some(snapshot) = snapshot.filter(|_| connected) else {
            self.mounted.clear();
            return;
        };
        self.metadata
            .update(cx, |metadata, cx| metadata.accept(&snapshot.plugins, cx));
        self.catalog_received = true;
        let packages: Vec<_> = self
            .metadata
            .read(cx)
            .entries
            .values()
            .filter_map(|info| {
                let extension = info.extension.as_ref()?;
                (info.summary.enabled
                    && extension.ui.iter().any(|entry| self.accepts(entry.slot))
                    && assistant.map_or(extension.scope == Scope::Host, |binding| {
                        binding.package.name == info.summary.name
                            || extension.scope == Scope::Host
                                && extension
                                    .desktop
                                    .as_ref()
                                    .is_some_and(|desktop| desktop.ui_shared)
                    }))
                .then_some((
                    info.summary.reference(),
                    extension.ui.clone(),
                    extension
                        .desktop
                        .as_ref()
                        .is_some_and(|desktop| desktop.ui_overlay),
                ))
            })
            .collect();
        self.mounted.retain(|_, mounted| {
            packages
                .iter()
                .any(|(package, _, _)| *package == mounted.package)
        });
        for (package, declarations, overlay) in packages {
            if self.mounted.contains_key(&package.name) {
                continue;
            }
            let bridge = Bridge::new(declarations);
            let content_sized = bridge
                .declarations
                .iter()
                .any(|entry| entry.kind == ui::Kind::Popover);
            let panel = cx.new(|cx| {
                let mut panel = Panel::contributions(
                    binding.clone(),
                    id,
                    self.surface,
                    bridge.clone(),
                    self.composer.clone(),
                    cx,
                );
                panel.content_sized = content_sized;
                panel
            });
            cx.emit(Event::Mounted(panel.clone()));
            let expected = package.clone();
            let observer = cx.observe_in(&panel, window, move |_, panel, window, cx| {
                if panel.read(cx).ready_for(&expected, cx) {
                    panel.update(cx, |panel, cx| panel.open(expected.clone(), window, cx));
                } else if panel.read(cx).selected.as_ref() == Some(&expected)
                    && panel.read(cx).available(&expected)
                    && panel.read(cx).metadata.read(cx).settled()
                {
                    // Retained controllers are not rendered while another scope is visible.
                    panel.update(cx, |panel, cx| panel.restore(window, cx));
                }
                cx.notify();
            });
            let mut updates = bridge.subscribe();
            let states = updates.borrow().clone();
            let expected = package.clone();
            let task = cx.spawn(async move |registry, cx| {
                while updates.changed().await.is_ok() {
                    let states = updates.borrow_and_update().clone();
                    if registry
                        .update(cx, |registry, cx| {
                            if let Some(mounted) = registry.mounted.get_mut(&expected.name)
                                && mounted.package == expected
                            {
                                mounted.states = states;
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            self.mounted.insert(
                package.name.clone(),
                Mounted {
                    overlay,
                    package,
                    bridge,
                    states,
                    panel,
                    _observer: observer,
                    _updates: task,
                },
            );
        }
    }

    pub fn entries(&self, slot: Slot, cx: &App) -> Vec<Entry> {
        if !self.accepts(slot) {
            return Vec::new();
        }
        let mut entries = Vec::new();
        for mounted in self.mounted.values() {
            for declaration in mounted
                .bridge
                .declarations
                .iter()
                .filter(|entry| entry.slot == slot)
            {
                let Some(state) = mounted
                    .states
                    .get(&declaration.id)
                    .filter(|state| state.visible)
                else {
                    continue;
                };
                let mut state = state.clone();
                state.enabled &= mounted.active(cx);
                entries.push(Entry {
                    key: Key {
                        node: self.node,
                        worktree: self.worktree,
                        session: self.session,
                        package: mounted.package.clone(),
                        id: declaration.id.clone(),
                    },
                    declaration: declaration.clone(),
                    state,
                });
            }
        }
        entries.sort_by(|left, right| {
            (left.declaration.order, &left.key.package.name, &left.key.id).cmp(&(
                right.declaration.order,
                &right.key.package.name,
                &right.key.id,
            ))
        });
        entries
    }

    pub(crate) fn commands(&self, cx: &App) -> Vec<Entry> {
        let mut entries: Vec<_> = [Slot::Composer, Slot::Context, Slot::Status, Slot::Commands]
            .into_iter()
            .flat_map(|slot| self.entries(slot, cx))
            .filter(|entry| entry.declaration.command.is_some())
            .collect();
        let mut counts = BTreeMap::new();
        for entry in &entries {
            *counts
                .entry(entry.declaration.command.as_ref().unwrap().name.clone())
                .or_insert(0) += 1;
        }
        for entry in &mut entries {
            entry.state.enabled &= counts[&entry.declaration.command.as_ref().unwrap().name] == 1;
        }
        entries.sort_by_key(|entry| {
            (
                entry.declaration.order,
                entry.key.package.name.clone(),
                entry.key.id.clone(),
            )
        });
        entries
    }

    /// Composer snapshots are read while a panel mounts; do not lease that panel.
    /// This is advisory state, not the activation or send-time availability check.
    pub(crate) fn message_command(&self, key: &Key, name: &str, cx: &App) -> bool {
        self.current(key).is_some_and(|mounted| {
            self.metadata
                .read(cx)
                .entries
                .get(&key.package.name)
                .is_some_and(|info| info.summary.enabled && info.summary.reference() == key.package)
                && mounted
                    .states
                    .get(&key.id)
                    .is_some_and(|state| state.visible && state.enabled)
                && mounted.bridge.declarations.iter().any(|entry| {
                    entry.id == key.id
                        && entry.command.as_ref().is_some_and(|command| {
                            command.name == name && command.kind == ui::CommandKind::Message
                        })
                })
        })
    }

    /// Commands use the same captured declaration and interaction as its control.
    pub(crate) fn activate(&mut self, key: &Key, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self
            .commands(cx)
            .into_iter()
            .find(|entry| entry.key == *key && entry.state.enabled)
        else {
            return;
        };
        match entry.declaration.kind {
            ui::Kind::Picker => {
                let owner = cx.entity();
                let key = key.clone();
                window.defer(cx, move |window, cx| picker::open(owner, key, window, cx));
            }
            ui::Kind::Toggle => self.invoke(
                key,
                ui::EventKind::Change,
                (!entry.state.value.as_bool().unwrap_or(false)).into(),
                window,
                cx,
            ),
            _ => self.invoke(
                key,
                ui::EventKind::Invoke,
                serde_json::Value::Null,
                window,
                cx,
            ),
        }
    }

    fn current(&self, key: &Key) -> Option<&Mounted> {
        self.mounted.get(&key.package.name).filter(|mounted| {
            self.node == key.node
                && self.worktree == key.worktree
                && self.session == key.session
                && mounted.package == key.package
        })
    }

    fn entry(&self, key: &Key, cx: &App) -> Option<Entry> {
        let declaration = self
            .current(key)?
            .bridge
            .declarations
            .iter()
            .find(|entry| entry.id == key.id)?;
        self.entries(declaration.slot, cx)
            .into_iter()
            .find(|entry| entry.key == *key)
    }

    fn invoke(
        &mut self,
        key: &Key,
        kind: ui::EventKind,
        value: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .current(key)
            .filter(|mounted| mounted.active(cx))
            .ok_or("contribution context changed")
            .and_then(|mounted| mounted.bridge.invoke(&key.id, kind, value));
        if let Err(error) = result {
            eprintln!("sailry-desktop: contribution action failed: {error}");
            crate::feedback::error(&crate::tr("plugins_failed"), error, window, cx);
            return;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod shortcut_tests;
