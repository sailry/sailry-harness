//! Dock routes retain the Node and worktree while resource pages are package-owned.
use super::*;

impl Shell {
    pub(crate) fn focus_terminal_pane(
        &mut self,
        node: NodeId,
        id: TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_plugin_pane(node, id, window, cx);
    }

    pub(crate) fn open_terminal_pane(
        &mut self,
        project: Option<ProjectId>,
        info: &Info,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tree) = info.worktree else { return };
        let Some(live) = &self.live else { return };
        let node = live.selected;
        let target = Target::Terminal(node, tree, info.id);
        self.terminals.remember(live.selected, info.id, project);
        if self.splits.read(cx).contains(target) {
            self.splits
                .update(cx, |splits, cx| splits.focus(target, cx));
        } else if let Some(snapshot) = live.view.snapshot.clone() {
            self.open_resource_pane(target, live.transport.clone(), &snapshot, window, cx);
        }
        self.focus_terminal_pane(node, info.id, window, cx);
    }

    pub(crate) fn restore_terminal_pane(
        &mut self,
        target: Target,
        transport: Arc<dyn Transport>,
        snapshot: &sailry_protocol::Snapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let extension = self
            .splits
            .read(cx)
            .pane(target)
            .and_then(|pane| pane.read(cx).extension.clone());
        self.restore_resource_pane(target, transport, snapshot, extension, window, cx)
    }

    pub(crate) fn restore_terminal_scope(
        &mut self,
        node: NodeId,
        tree: WorktreeId,
        terminal: TerminalId,
        cx: &mut Context<Self>,
    ) {
        if let Some(live) = &mut self.live {
            if live.selected != node {
                live.select(node, cx);
            }
            if let Some(project) = self.terminals.owners.get(&(node, terminal)).copied() {
                live.project = project;
                live.unassigned_worktree = project.is_none().then_some(tree);
                if let Some(project) = project {
                    live.worktree_choices.insert((node, project), tree);
                }
            }
        }
        self.terminals.selected.insert((node, tree), terminal);
    }

    pub(crate) fn close_terminal_pane(
        &mut self,
        node: NodeId,
        tree: WorktreeId,
        terminal: TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_plugin_pane(Target::Terminal(node, tree, terminal), window, cx);
    }
}
