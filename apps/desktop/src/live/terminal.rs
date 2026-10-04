//! Shared terminal identity and dock navigation; packages own resource actions and pages.
use super::*;
use crate::{panes::Target, preview::Page};
use gpui_kit::component::v_flex;
use sailry_protocol::{
    TerminalId,
    plugin::desktop::ResourceKind,
    terminal::{Info, Status},
};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

mod panes;
mod row;
#[cfg(test)]
mod tests;

#[derive(Default)]
pub(crate) struct State {
    selected: BTreeMap<(NodeId, WorktreeId), TerminalId>,
    owners: BTreeMap<(NodeId, TerminalId), Option<ProjectId>>,
    pending: BTreeSet<(NodeId, WorktreeId)>,
}

impl State {
    pub(crate) fn remember(
        &mut self,
        node: NodeId,
        terminal: TerminalId,
        project: Option<ProjectId>,
    ) {
        self.owners.insert((node, terminal), project);
    }

    pub(super) fn prune(&mut self, node: NodeId, snapshot: &sailry_protocol::Snapshot) {
        self.selected.retain(|(owner, tree), id| {
            *owner != node
                || snapshot.terminals.iter().any(|info| {
                    info.id == *id && info.worktree == Some(*tree) && info.status != Status::Closed
                })
        });
        self.owners.retain(|(owner, id), _| {
            *owner != node
                || snapshot
                    .terminals
                    .iter()
                    .any(|info| info.id == *id && info.status != Status::Closed)
        });
    }
}

impl Shell {
    pub(crate) fn close_active_terminal(
        &mut self,
        _: &crate::shell::shortcuts::CloseFocused,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != Page::Terminal {
            cx.propagate();
            return;
        }
        if let Some(target @ Target::Terminal(..)) = self.splits.read(cx).active {
            self.close_pane(target, window, cx);
        }
    }

    #[cfg(test)]
    pub(crate) fn open_live_terminals(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.guard_file_navigation(window, cx, |shell, window, cx| {
            shell.open_live_terminals(window, cx)
        }) {
            return;
        }
        self.park_session_panel(window, cx);
        self.page = Page::Terminal;
        self.sync_session_panel(window, cx);
        cx.notify();
    }

    pub(crate) fn terminal_action(
        &mut self,
        close: Option<TerminalId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(live) = &self.live else { return };
        let Some(tree) = live.selected_worktree() else {
            return;
        };
        let key = (live.selected, tree.id);
        if let Some(id) = close {
            self.close_terminal_pane(key.0, key.1, id, window, cx);
            return;
        }
        if !live.view.connected || !self.terminals.pending.insert(key) {
            return;
        }
        let Some(registry) = self.worktree_plugins(key.1, window, cx) else {
            self.terminals.pending.remove(&key);
            return;
        };
        if registry.read(cx).ready(cx) {
            registry.update(cx, |registry, cx| {
                registry.create(ResourceKind::Terminal, window, cx)
            });
            self.terminals.pending.remove(&key);
            return;
        }
        cx.spawn_in(window, async move |shell, cx| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let done = shell
                    .update_in(cx, |shell, window, cx| {
                        if shell.live.as_ref().is_none_or(|live| {
                            live.selected != key.0
                                || live.selected_worktree().is_none_or(|tree| tree.id != key.1)
                        }) {
                            shell.terminals.pending.remove(&key);
                            return true;
                        }
                        shell.worktree_plugins(key.1, window, cx);
                        if registry.read(cx).ready(cx) {
                            registry.update(cx, |registry, cx| {
                                registry.create(ResourceKind::Terminal, window, cx)
                            });
                            shell.terminals.pending.remove(&key);
                            return true;
                        }
                        if Instant::now() >= deadline {
                            shell.terminals.pending.remove(&key);
                            crate::feedback::error(
                                &tr("plugins_failed"),
                                "resource action is unavailable",
                                window,
                                cx,
                            );
                            return true;
                        }
                        false
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(20))
                    .await;
            }
        })
        .detach();
    }

    pub(crate) fn reveal_terminal(
        &mut self,
        project: Option<ProjectId>,
        info: &Info,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(live) = &mut self.live
            && let Some(worktree) = info.worktree
        {
            live.project = project;
            live.unassigned_worktree = project.is_none().then_some(worktree);
            if let Some(project) = project {
                live.worktree_choices
                    .insert((live.selected, project), worktree);
            }
            self.terminals
                .selected
                .insert((live.selected, worktree), info.id);
        }
        self.park_session_panel(window, cx);
        self.page = Page::Terminal;
        self.open_terminal_pane(project, info, window, cx);
        self.sync_session_panel(window, cx);
        cx.notify();
    }

    pub(crate) fn terminal_workspace_visible(&self, cx: &App) -> bool {
        let Some(live) = &self.live else { return false };
        matches!(self.splits.read(cx).active, Some(target @ Target::Terminal(node, tree, _))
            if !self.plugin_panes.contains(target) || (node == live.selected && live.selected_worktree().is_some_and(|selected| selected.id == tree)))
    }

    pub(crate) fn live_terminal_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.terminal_workspace_visible(cx) {
            return self.split_content(window, cx);
        }
        let Some(live) = &self.live else {
            return div().into_any_element();
        };
        let Some(tree) = live.selected_worktree() else {
            return div().into_any_element();
        };
        let key = (live.selected, tree.id);
        let project = tree.project;
        let terminals: Vec<_> = live
            .view
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.terminals)
            .filter(|info| info.worktree == Some(key.1) && info.status != Status::Closed)
            .cloned()
            .collect();
        let selected = self
            .terminals
            .selected
            .get(&key)
            .and_then(|id| terminals.iter().position(|info| info.id == *id))
            .unwrap_or(0);
        if let Some(info) = terminals.get(selected) {
            self.open_terminal_pane(project, info, window, cx);
            return self.split_content(window, cx);
        }
        let action = self
            .worktree_plugins(key.1, window, cx)
            .and_then(|registry| {
                registry.update(cx, |registry, cx| {
                    registry.creation(ResourceKind::Terminal, cx).map(|entry| {
                        registry.control(entry, crate::plugins::contributions::Form::Project, cx)
                    })
                })
            });
        v_flex()
            .id("live-terminal-page")
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(tr("workspace_no_terminal"))
            .children(action)
            .into_any_element()
    }
}
