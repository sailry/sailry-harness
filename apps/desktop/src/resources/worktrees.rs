//! Release clean presentation resources after Node removes their worktree.
use super::*;
use sailry_protocol::{NodeId, WorktreeId};

impl Shell {
    pub(crate) fn worktree_has_drafts(&self, scope: (NodeId, WorktreeId), cx: &App) -> bool {
        self.documents.has_unsaved(scope, cx)
    }

    pub(crate) fn release_worktree(
        &mut self,
        scope: (NodeId, WorktreeId),
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.documents.release(scope, cx);
        for panel in self.session_scope.panels.values_mut() {
            if matches!(&panel.resource, Some(SideResource::Plugin(panel)) if panel.read(cx).resource_scope(cx) == Some(scope))
            {
                panel.resource = None;
            }
        }
        if matches!(&self.side_resource, Some(SideResource::Plugin(panel)) if panel.read(cx).resource_scope(cx) == Some(scope))
        {
            self.close_resource_panel(cx);
        }
        cx.notify();
    }
}
