//! Packaged pages share the existing resource identity, Kit dock, and recovery path.
mod sdk;
mod titles;
pub(super) use sdk::module;
use titles::{ResourceTitle, Title};

use super::Panel;
use crate::{
    conversation::live::Binding,
    panes::{Extension, Target},
    shell::Shell,
};
use gpui_kit::*;
use sailry_protocol::{NodeId, TerminalId, WorktreeId, plugin::Reference};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum Resource {
    Terminal(TerminalId),
}

#[derive(Default)]
pub(crate) struct State {
    pub(super) panels: BTreeMap<Target, Entity<Panel>>,
    titles: BTreeMap<(Target, String), Title>,
    closing: BTreeSet<Target>,
}

impl State {
    pub(crate) fn contains(&self, target: Target) -> bool {
        self.panels.contains_key(&target)
    }

    #[cfg(test)]
    pub(crate) fn panel_id(&self, target: Target) -> Option<EntityId> {
        self.panels.get(&target).map(Entity::entity_id)
    }

    #[cfg(test)]
    pub(crate) fn terminal(
        &self,
        target: Target,
        cx: &App,
    ) -> Option<Entity<crate::terminal::View>> {
        self.panels
            .get(&target)?
            .read(cx)
            .mounted
            .as_ref()?
            .terminals
            .first()
    }
}

#[derive(Clone)]
pub(super) struct Events {
    sender: tokio::sync::mpsc::Sender<&'static str>,
    receiver: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<&'static str>>>,
}

pub(super) struct Scope {
    pub resource: Option<Resource>,
    pub events: Events,
}

impl Default for Events {
    fn default() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        Self {
            sender,
            receiver: Arc::new(tokio::sync::Mutex::new(receiver)),
        }
    }
}

pub(super) struct Event {
    package: Reference,
    action: Action,
}

pub(super) enum Action {
    Open {
        resource: Resource,
        title: String,
    },
    Update {
        title: Option<String>,
        busy: Option<bool>,
    },
    Close,
    Titles(Vec<ResourceTitle>),
}
impl EventEmitter<Event> for Panel {}

impl Panel {
    fn target(&self, resource: Resource) -> Option<Target> {
        match resource {
            Resource::Terminal(id) => Some(Target::Terminal(
                self.binding.client.target(),
                self.binding.worktree?,
                id,
            )),
        }
    }

    fn permits_pane(&self, resource: Resource, cx: &App) -> bool {
        use sailry_protocol::{plugin::Action, terminal::Status};
        let Some(package) = &self.selected else {
            return false;
        };
        let metadata = self.metadata.read(cx);
        let Some(extension) = metadata
            .entries
            .get(&package.name)
            .and_then(|info| info.extension.as_ref())
        else {
            return false;
        };
        match resource {
            Resource::Terminal(id) => {
                self.binding.worktree.is_some()
                    && [Action::ReadTerminals, Action::ControlTerminals]
                        .iter()
                        .all(|action| extension.actions.contains(action))
                    && self.snapshot.borrow().as_ref().is_some_and(|snapshot| {
                        snapshot.terminals.iter().any(|info| {
                            info.id == id
                                && info.worktree == self.binding.worktree
                                && info.status != Status::Closed
                        })
                    })
            }
        }
    }

    fn focus_pane(&mut self, window: &mut Window, cx: &mut App) {
        if let (Some(Resource::Terminal(id)), Some(mounted)) = (self.pane, &self.mounted) {
            mounted.terminals.focus(id, window, cx);
        } else {
            self.pane_focus = true;
        }
    }
}

impl Shell {
    pub(super) fn observe_plugin_panes(
        panel: &Entity<Panel>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        Self::observe_resource_titles(panel, cx);
        cx.subscribe_in(panel, window, |shell, panel, event: &Event, window, cx| {
            if panel.read(cx).selected.as_ref() != Some(&event.package)
                || !panel.read(cx).connected
                || panel.read(cx).loading
                || !panel.read(cx).available(&event.package)
                || panel
                    .read(cx)
                    .mounted
                    .as_ref()
                    .is_none_or(|mounted| !mounted.active())
            {
                return;
            }
            match &event.action {
                Action::Open { resource, title } => {
                    shell.open_plugin_pane(
                        panel.clone(),
                        event.package.clone(),
                        *resource,
                        title.clone(),
                        window,
                        cx,
                    );
                }
                Action::Update { title, busy } => {
                    let Some(target) = shell
                        .plugin_panes
                        .panels
                        .iter()
                        .find_map(|(target, current)| (current == panel).then_some(*target))
                    else {
                        return;
                    };
                    if let Some(pane) = shell.splits.read(cx).pane(target).cloned() {
                        pane.update(cx, |pane, cx| {
                            if let Some(title) = title {
                                pane.title = title.clone().into();
                            }
                            if let Some(busy) = busy {
                                pane.closing = *busy;
                            }
                            cx.notify();
                        });
                    }
                    cx.notify();
                }
                Action::Close => {
                    if let Some(target) = shell
                        .plugin_panes
                        .panels
                        .iter()
                        .find_map(|(target, current)| (current == panel).then_some(*target))
                    {
                        shell.remove_pane(target, window, cx);
                    }
                }
                Action::Titles(titles) => {
                    shell.publish_resource_titles(panel, &event.package, titles, cx)
                }
            }
        })
        .detach();
    }

    fn open_plugin_pane(
        &mut self,
        source: Entity<Panel>,
        package: Reference,
        resource: Resource,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = source.read(cx);
        if !state.connected
            || state.selected.as_ref() != Some(&package)
            || !state.permits_pane(resource, cx)
        {
            return;
        }
        let Some(target) = state.target(resource) else {
            return;
        };
        let binding = state.binding.clone();
        if state.surface == sailry_protocol::plugin::desktop::Surface::Project
            && self.live.as_ref().is_none_or(|live| {
                live.selected != binding.client.target()
                    || live.selected_worktree().map(|tree| tree.id) != binding.worktree
            })
        {
            return;
        }
        if self.guard_file_navigation(window, cx, {
            let source = source.clone();
            let package = package.clone();
            let title = title.clone();
            move |shell, window, cx| {
                shell.open_plugin_pane(
                    source.clone(),
                    package.clone(),
                    resource,
                    title.clone(),
                    window,
                    cx,
                )
            }
        }) {
            return;
        }
        self.park_session_panel(window, cx);
        let extension = Extension {
            package: package.name,
        };
        let existing = self
            .splits
            .read(cx)
            .pane(target)
            .is_some_and(|pane| pane.read(cx).extension.as_ref() == Some(&extension))
            && self.plugin_panes.panels.contains_key(&target);
        if !existing {
            let project = binding.project;
            let project_name = binding.project_name.clone();
            let panel = self.mount_plugin_pane(
                target,
                binding,
                Some(extension.clone()),
                resource,
                window,
                cx,
            );
            self.splits.update(cx, |splits, cx| {
                splits.open(target, title.into(), project_name, panel.into(), window, cx);
                splits
                    .pane(target)
                    .unwrap()
                    .update(cx, |pane, _| pane.extension = Some(extension));
            });
            if let Target::Terminal(node, _, id) = target {
                self.terminals.remember(node, id, project);
            }
        }
        self.splits
            .update(cx, |splits, cx| splits.focus(target, cx));
        self.activate_pane(target, window, cx);
    }

    fn mount_plugin_pane(
        &mut self,
        target: Target,
        binding: Binding,
        extension: Option<Extension>,
        resource: Resource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Panel> {
        let panel = cx.new(|cx| Panel::pane(binding, resource, cx));
        if self.splits.read(cx).active == Some(target) {
            panel.update(cx, |panel, _| panel.pane_focus = true);
        }
        Self::observe_plugin_conversations(&panel, window, cx);
        cx.observe_in(&panel, window, move |shell, panel, window, cx| {
            let state = panel.read(cx);
            let unavailable = state
                .mounted
                .as_ref()
                .is_none_or(|mounted| !mounted.active())
                || state.loading
                || !state.connected
                || state
                    .selected
                    .as_ref()
                    .is_none_or(|package| !state.available(package));
            let failed = !state.loading
                && (state.error.is_some()
                    || (state.snapshot.borrow().is_some() && !state.connected)
                    || state.selected.as_ref().is_some_and(|package| !state.available(package))
                    || state.mounted.as_ref().is_some_and(|mounted| !mounted.active()));
            if failed {
                shell.plugin_panes.closing.remove(&target);
            }
            if state.selected.is_none() && state.connected && state.metadata.read(cx).settled() {
                let package = state.metadata.read(cx).entries.values()
                    .filter(|info| info.summary.enabled)
                    .find(|info| extension.as_ref().map_or_else(|| {
                        info.extension.as_ref().and_then(|extension| extension.desktop.as_ref())
                            .is_some_and(|desktop| desktop.renderers.iter().any(|renderer| {
                                matches!((renderer.resource, resource),
                                    (sailry_protocol::plugin::desktop::ResourceKind::Terminal, Resource::Terminal(_)))
                            }))
                    }, |extension| info.summary.name == extension.package))
                    .map(|info| info.summary.reference());
                if let Some(package) = package {
                    if let Some(pane) = shell.splits.read(cx).pane(target).cloned() {
                        pane.update(cx, |pane, _| pane.extension = Some(Extension { package: package.name.clone() }));
                    }
                    panel.update(cx, |panel, cx| panel.open(package, window, cx));
                } else if shell.plugin_panes.closing.remove(&target) {
                    shell.remove_pane(target, window, cx);
                }
            }
            if !unavailable && shell.plugin_panes.closing.remove(&target) {
                let _ = panel.read(cx).pane_events.sender.try_send("close");
            }
            if unavailable && let Some(pane) = shell.splits.read(cx).pane(target).cloned() {
                pane.update(cx, |pane, cx| {
                    if pane.closing {
                        pane.closing = false;
                        cx.notify();
                    }
                });
            }
            if shell.splits.read(cx).active == Some(target) {
                cx.notify();
            }
        })
        .detach();
        self.plugin_panes.panels.insert(target, panel.clone());
        panel
    }

    pub(crate) fn focus_plugin_pane(
        &self,
        node: NodeId,
        id: TerminalId,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let panel = self.plugin_panes.panels.iter().find_map(|(target, panel)| {
            matches!(target, Target::Terminal(owner, _, terminal) if *owner == node && *terminal == id).then_some(panel)
        });
        if let Some(panel) = panel {
            panel.update(cx, |panel, cx| panel.focus_pane(window, cx));
            true
        } else {
            false
        }
    }

    pub(crate) fn clear_plugin_focus(&self, active: Option<Target>, cx: &mut App) {
        for (target, panel) in &self.plugin_panes.panels {
            if Some(*target) != active {
                panel.update(cx, |panel, _| {
                    panel.pane_focus = false;
                    if let Some(mounted) = &panel.mounted {
                        mounted.terminals.clear_focus();
                    }
                });
            }
        }
    }

    pub(crate) fn close_plugin_pane(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(pane) = self.splits.read(cx).pane(target) else {
            return false;
        };
        if pane.read(cx).extension.is_none() && !self.plugin_panes.panels.contains_key(&target) {
            return false;
        }
        if let Some(panel) = self.plugin_panes.panels.get(&target).filter(|panel| {
            let panel = panel.read(cx);
            panel.connected
                && !panel.loading
                && panel
                    .selected
                    .as_ref()
                    .is_some_and(|package| panel.available(package))
                && panel
                    .mounted
                    .as_ref()
                    .is_some_and(|mounted| mounted.active())
        }) {
            let _ = panel.read(cx).pane_events.sender.try_send("close");
        } else if self.plugin_panes.panels.get(&target).is_some_and(|panel| {
            let panel = panel.read(cx);
            panel.snapshot.borrow().is_none()
                || (panel.connected && (panel.loading || panel.selected.is_none()))
        }) {
            self.plugin_panes.closing.insert(target);
        } else {
            // An unavailable renderer can release its view, never execute a fallback command.
            self.remove_pane(target, window, cx);
        }
        true
    }

    pub(crate) fn release_plugin_pane(&mut self, target: Target) {
        self.plugin_panes.panels.remove(&target);
        self.plugin_panes.closing.remove(&target);
    }

    pub(crate) fn open_resource_pane(
        &mut self,
        target: Target,
        transport: Arc<dyn sailry_link::Transport>,
        snapshot: &sailry_protocol::Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Target::Terminal(node, tree, terminal) = target else {
            return;
        };
        self.restore_resource_pane(
            Target::Terminal(node, tree, terminal),
            transport,
            snapshot,
            None,
            window,
            cx,
        );
    }

    pub(crate) fn restore_resource_pane(
        &mut self,
        target: Target,
        transport: Arc<dyn sailry_link::Transport>,
        snapshot: &sailry_protocol::Snapshot,
        extension: Option<Extension>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Target::Terminal(node, tree, terminal) = target else {
            return false;
        };
        let Some(worktree) = snapshot
            .worktrees
            .iter()
            .find(|worktree| worktree.id == tree)
        else {
            return false;
        };
        if !snapshot.terminals.iter().any(|info| {
            info.id == terminal
                && info.worktree == Some(tree)
                && info.status != sailry_protocol::terminal::Status::Closed
        }) {
            return false;
        }
        let services = cx.global::<crate::backend::Services>();
        let project_name: SharedString = snapshot
            .projects
            .iter()
            .find(|project| Some(project.id) == worktree.project)
            .map(|project| project.name.clone().into())
            .unwrap_or_default();
        let binding = Binding {
            client: Arc::new(sailry_client::Client::new(transport)),
            defaults: Arc::new(sailry_client::Client::new(services.local.clone())),
            runtime: services.runtime.clone(),
            project: worktree.project,
            worktree: Some(tree),
            host: self.live.as_ref().unwrap().name(node),
            project_name: project_name.clone(),
            branch: worktree.path.clone().into(),
        };
        let panel = self.mount_plugin_pane(
            target,
            binding,
            extension,
            Resource::Terminal(terminal),
            window,
            cx,
        );
        self.terminals.remember(node, terminal, worktree.project);
        let title = self.terminal_title(node, tree, terminal, cx);
        self.splits.update(cx, |splits, cx| {
            if splits.contains(target) {
                splits.hydrate(target, title, project_name, panel.into(), cx);
            } else {
                splits.open(target, title, project_name, panel.into(), window, cx);
            }
        });
        true
    }
}
