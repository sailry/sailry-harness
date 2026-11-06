//! Worktree selection remains common presentation state.
use super::*;
use sailry_protocol::{Snapshot, Worktree};

#[cfg(test)]
mod tests;

impl State {
    pub(crate) fn restore_scope(
        &mut self,
        project: Option<ProjectId>,
        worktree: Option<WorktreeId>,
    ) {
        (self.project, self.unassigned_worktree) =
            restored_scope(project, worktree, self.view.snapshot.as_ref());
        if let (Some(project), Some(worktree)) = (self.project, worktree) {
            self.worktree_choices
                .insert((self.selected, project), worktree);
        }
    }

    pub(crate) fn select_project_worktree(&mut self, project: ProjectId, worktree: WorktreeId) {
        self.project = Some(project);
        self.unassigned_worktree = None;
        self.worktree_choices
            .insert((self.selected, project), worktree);
    }

    pub(crate) fn accept_view(&mut self, view: View) {
        if let Some(snapshot) = &view.snapshot {
            let previous = self.view.snapshot.as_ref();
            if self
                .project
                .is_some_and(|project| project_removed(project, snapshot, previous))
            {
                self.project = None;
                self.unassigned_worktree = None;
            }
            retain_choices(&mut self.worktree_choices, snapshot, previous);
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
        if !self
            .view
            .snapshot
            .as_ref()?
            .projects
            .iter()
            .any(|entry| entry.id == project)
        {
            return None;
        }
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

fn restored_scope(
    project: Option<ProjectId>,
    worktree: Option<WorktreeId>,
    snapshot: Option<&Snapshot>,
) -> (Option<ProjectId>, Option<WorktreeId>) {
    let selected = project.filter(|project| {
        snapshot.is_none_or(|snapshot| snapshot.projects.iter().any(|entry| entry.id == *project))
    });
    (selected, if project.is_none() { worktree } else { None })
}

fn project_removed(project: ProjectId, snapshot: &Snapshot, previous: Option<&Snapshot>) -> bool {
    !snapshot.projects.iter().any(|entry| entry.id == project)
        && (previous
            .is_some_and(|snapshot| snapshot.projects.iter().any(|entry| entry.id == project))
            || snapshot
                .worktrees
                .iter()
                .any(|tree| tree.project == Some(project)))
}

fn retain_choices(
    choices: &mut BTreeMap<(NodeId, ProjectId), WorktreeId>,
    snapshot: &Snapshot,
    previous: Option<&Snapshot>,
) {
    choices.retain(|(node, project), id| {
        *node != snapshot.node
            || !project_removed(*project, snapshot, previous)
                && (snapshot.worktrees.iter().any(|tree| tree.project == Some(*project) && tree.id == *id)
                    // A creation response can arrive before its subscription update.
                    || previous.is_none_or(|snapshot| !snapshot.worktrees.iter().any(|tree| tree.id == *id)))
    });
}
