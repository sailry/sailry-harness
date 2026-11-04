use super::{Owner, Project, State};
use crate::tr;
use gpui_kit::SharedString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Repository {
    Directory,
    Unborn,
    Ready,
}

impl State {
    pub fn initialize_repository(
        &mut self,
        id: usize,
        expected: &Project,
    ) -> Result<(), &'static str> {
        let project = self.projects.get(&id).ok_or("project_missing")?;
        if project.host != expected.host || project.path != expected.path {
            return Err("project_changed");
        }
        if project.repository != Repository::Directory {
            return Ok(());
        }
        let root = self
            .worktrees
            .values_mut()
            .find(|tree| tree.project == id && tree.main)
            .ok_or("worktree_remove_missing")?;
        root.branch = tr("composer_branch_main");
        self.projects.get_mut(&id).unwrap().repository = Repository::Unborn;
        Ok(())
    }

    pub fn new_worktree_reason(&self, project: usize) -> Option<&'static str> {
        match self
            .projects
            .get(&project)
            .map(|project| project.repository)
        {
            None => Some("worktree_missing_project"),
            Some(Repository::Directory) => Some("git_directory_hint"),
            Some(Repository::Unborn) => Some("git_worktree_requires_commit"),
            Some(Repository::Ready) => None,
        }
    }

    pub fn branch_label(&self, owner: Owner) -> SharedString {
        if self.projects[&owner.project].repository == Repository::Directory {
            tr("git_directory")
        } else {
            let branch = self.worktrees[&owner.worktree].branch.clone();
            if branch.is_empty() {
                tr("git_detached")
            } else {
                branch
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_and_staleness() {
        for (scenario, check) in [
            (
                "idempotent_initialization",
                (|_scenario: &str| {
                    let mut state = State::default();
                    let id = state
                        .save_project(0, None, "Directory", "/preview/directory")
                        .unwrap();
                    let owner = state.project_owner(id).unwrap();
                    let session = state.create_session(owner);
                    let terminal = state.create_terminal(owner);
                    let expected = state.projects[&id].clone();
                    assert_eq!(
                        expected.repository,
                        Repository::Directory,
                        "idempotent initialization"
                    );
                    assert!(
                        state.worktrees[&owner.worktree].branch.is_empty(),
                        "idempotent initialization"
                    );
                    assert_eq!(
                        state.add_worktree(id, "main", "feature", "/preview/new"),
                        Err("git_directory_hint"),
                        "idempotent initialization"
                    );
                    state.initialize_repository(id, &expected).unwrap();
                    assert_eq!(
                        state.projects[&id].repository,
                        Repository::Unborn,
                        "idempotent initialization"
                    );
                    assert!(!state.projects[&id].trusted, "idempotent initialization");
                    assert_eq!(
                        state.sessions[&session].owner, owner,
                        "idempotent initialization"
                    );
                    assert_eq!(
                        state.terminals[&terminal].owner, owner,
                        "idempotent initialization"
                    );
                    assert_eq!(
                        state.branch_label(owner),
                        tr("composer_branch_main"),
                        "idempotent initialization"
                    );
                    assert_eq!(
                        state.add_worktree(id, "main", "feature", "/preview/new"),
                        Err("git_worktree_requires_commit"),
                        "idempotent initialization"
                    );
                    assert_eq!(
                        state.initialize_repository(id, &expected),
                        Ok(()),
                        "idempotent initialization"
                    );
                    assert_eq!(state.worktrees.len(), 5, "idempotent initialization");
                }) as fn(&str),
            ),
            (
                "rejects_stale_targets",
                (|_scenario: &str| {
                    let mut state = State::default();
                    let id = state
                        .save_project(1, None, "Directory", "/preview/directory")
                        .unwrap();
                    let expected = state.projects[&id].clone();
                    state
                        .save_project(1, Some((id, &expected)), "Changed", "/preview/changed")
                        .unwrap();
                    assert_eq!(
                        state.initialize_repository(id, &expected),
                        Err("project_changed"),
                        "rejects stale targets"
                    );
                    assert_eq!(
                        state.projects[&id].repository,
                        Repository::Directory,
                        "rejects stale targets"
                    );
                    let removal = state.project_removal(id).unwrap();
                    state.remove_project(&removal).unwrap();
                    assert_eq!(
                        state.initialize_repository(id, &expected),
                        Err("project_missing"),
                        "rejects stale targets"
                    );
                    assert_eq!(
                        state.projects[&0].repository,
                        Repository::Ready,
                        "rejects stale targets"
                    );
                }),
            ),
            (
                "clears_changed_roots",
                (|_scenario: &str| {
                    let mut state = State::default();
                    let id = state
                        .save_project(0, None, "Directory", "/preview/directory")
                        .unwrap();
                    let original = state.projects[&id].clone();
                    state.initialize_repository(id, &original).unwrap();
                    let initialized = state.projects[&id].clone();
                    state
                        .save_project(0, Some((id, &initialized)), "Moved", "/preview/moved")
                        .unwrap();
                    assert_eq!(
                        state.projects[&id].repository,
                        Repository::Directory,
                        "clears changed roots"
                    );
                    assert_ne!(
                        state.projects[&id].active_worktree, initialized.active_worktree,
                        "clears changed roots"
                    );
                    assert!(
                        state.worktrees[&state.projects[&id].active_worktree]
                            .branch
                            .is_empty(),
                        "clears changed roots"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
