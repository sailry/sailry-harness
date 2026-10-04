//! Worktree selection remains common presentation state.
use super::*;
use sailry_protocol::Worktree;

impl State {
    pub(crate) fn select_project_worktree(&mut self, project: ProjectId, worktree: WorktreeId) {
        self.project = Some(project);
        self.unassigned_worktree = None;
        self.worktree_choices
            .insert((self.selected, project), worktree);
    }

    pub(crate) fn accept_view(&mut self, view: View) {
        if let Some(snapshot) = &view.snapshot {
            let previous = self.view.snapshot.as_ref();
            self.worktree_choices.retain(|(node, project), id| {
                *node != snapshot.node
                    || snapshot.worktrees.iter().any(|tree| tree.project == Some(*project) && tree.id == *id)
                    // A creation response can arrive before its subscription update.
                    || previous.is_some_and(|snapshot| !snapshot.worktrees.iter().any(|tree| tree.id == *id))
            });
        }
        self.view = view;
    }

    pub(crate) fn selected_worktree(&self) -> Option<&Worktree> {
        if let Some(project) = self.project {
            self.project_worktree(project)
        } else {
            let id = self.unassigned_worktree?;
            self.view
                .snapshot
                .as_ref()?
                .worktrees
                .iter()
                .find(|tree| tree.id == id && tree.project.is_none())
        }
    }

    pub(crate) fn project_worktree(&self, project: ProjectId) -> Option<&Worktree> {
        let choice = self.worktree_choices.get(&(self.selected, project));
        self.view
            .snapshot
            .as_ref()?
            .worktrees
            .iter()
            .find(|worktree| {
                worktree.project == Some(project)
                    && choice.map_or(worktree.main, |id| *id == worktree.id)
            })
    }
}
