use super::{Owner, State, Worktree};
use gpui_kit::SharedString;

#[derive(Clone)]
pub(crate) struct Removal {
    pub owner: Owner,
    pub tree: Worktree,
    project_path: SharedString,
}

impl State {
    pub fn removal(&self, owner: Owner) -> Option<Removal> {
        self.contains(owner).then(|| Removal {
            owner,
            tree: self.worktrees[&owner.worktree].clone(),
            project_path: self.projects[&owner.project].path.clone(),
        })
    }

    pub fn removal_reason(&self, owner: Owner) -> Option<&'static str> {
        if !self.contains(owner) {
            return Some("worktree_remove_missing");
        }
        let tree = &self.worktrees[&owner.worktree];
        if tree.main {
            Some("worktree_remove_main")
        } else if tree.locked {
            Some("worktree_remove_locked")
        } else if !self.projects[&owner.project].trusted {
            Some("worktree_remove_untrusted")
        } else if tree.busy {
            Some("worktree_remove_busy")
        } else if tree.dirty {
            Some("worktree_remove_dirty")
        } else if tree.branch.trim().is_empty() {
            Some("worktree_remove_detached")
        } else if self.sessions.values().any(|session| session.owner == owner) {
            // Archived sessions still own their history and routes.
            Some("worktree_remove_sessions")
        } else if self
            .terminals
            .values()
            .any(|terminal| terminal.owner == owner)
        {
            Some("worktree_remove_terminals")
        } else {
            None
        }
    }

    pub fn remove_worktree(&mut self, expected: &Removal) -> Result<(), &'static str> {
        let owner = expected.owner;
        let current = self.removal(owner).ok_or("worktree_remove_missing")?;
        if current.tree != expected.tree || current.project_path != expected.project_path {
            return Err("worktree_remove_changed");
        }
        if let Some(reason) = self.removal_reason(owner) {
            return Err(reason);
        }
        let main = self
            .worktrees
            .iter()
            .find(|(_, tree)| tree.project == owner.project && tree.main)
            .map(|(&id, _)| id)
            .ok_or("worktree_remove_missing")?;
        self.worktrees.remove(&owner.worktree);
        let project = self.projects.get_mut(&owner.project).unwrap();
        if project.active_worktree == owner.worktree {
            project.active_worktree = main;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty(state: &mut State, project: usize) -> Owner {
        state
            .add_worktree(project, "main", "feature/remove", "/preview/remove")
            .unwrap()
    }

    #[test]
    fn ownership_and_atomicity() {
        for (scenario, check) in [
            (
                "protected_states",
                (|_scenario: &str| {
                    let mut state = State::default();
                    assert_eq!(
                        state.removal_reason(state.owner(0)),
                        Some("worktree_remove_main"),
                        "protected states"
                    );
                    let owner = empty(&mut state, 0);
                    for (reason, main, locked, busy, dirty, branch) in [
                        (
                            "worktree_remove_main",
                            true,
                            false,
                            false,
                            false,
                            "feature/remove",
                        ),
                        (
                            "worktree_remove_locked",
                            false,
                            true,
                            false,
                            false,
                            "feature/remove",
                        ),
                        (
                            "worktree_remove_busy",
                            false,
                            false,
                            true,
                            false,
                            "feature/remove",
                        ),
                        (
                            "worktree_remove_dirty",
                            false,
                            false,
                            false,
                            true,
                            "feature/remove",
                        ),
                        ("worktree_remove_detached", false, false, false, false, ""),
                    ] {
                        let tree = state.worktrees.get_mut(&owner.worktree).unwrap();
                        tree.main = main;
                        tree.locked = locked;
                        tree.busy = busy;
                        tree.dirty = dirty;
                        tree.branch = branch.into();
                        let expected = state.removal(owner).unwrap();
                        assert_eq!(
                            state.remove_worktree(&expected),
                            Err(reason),
                            "protected states"
                        );
                        assert!(state.contains(owner), "protected states");
                    }
                    state.projects.get_mut(&0).unwrap().trusted = false;
                    let expected = state.removal(owner).unwrap();
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Err("worktree_remove_untrusted"),
                        "protected states"
                    );
                    assert!(
                        state.removal(Owner { host: 1, ..owner }).is_none(),
                        "protected states"
                    );
                }) as fn(&str),
            ),
            (
                "retains_archived_owners",
                (|_scenario: &str| {
                    let mut state = State::default();
                    let owner = empty(&mut state, 0);
                    let expected = state.removal(owner).unwrap();
                    let key = state.create_session(owner);
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Err("worktree_remove_sessions"),
                        "retains archived owners"
                    );
                    state.sessions.get_mut(&key).unwrap().archived = true;
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Err("worktree_remove_sessions"),
                        "retains archived owners"
                    );
                    state.sessions.remove(&key);
                    let terminal = state.create_terminal(owner);
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Err("worktree_remove_terminals"),
                        "retains archived owners"
                    );
                    state.terminals.remove(&terminal);
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Ok(()),
                        "retains archived owners"
                    );
                }),
            ),
            (
                "selected_fallback_and_identity",
                (|_scenario: &str| {
                    let mut state = State::default();
                    let owner = empty(&mut state, 0);
                    state.select(owner);
                    let remote = state.owner(1);
                    let expected = state.removal(owner).unwrap();
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Ok(()),
                        "selected fallback and identity"
                    );
                    assert_eq!(state.owner(0).worktree, 0, "selected fallback and identity");
                    assert_eq!(state.owner(1), remote, "selected fallback and identity");
                    assert_eq!(
                        state.sessions[&(0, 0)].owner.worktree,
                        0,
                        "selected fallback and identity"
                    );
                    assert_eq!(
                        state.terminals[&(0, 0)].owner.worktree,
                        0,
                        "selected fallback and identity"
                    );
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Err("worktree_remove_missing"),
                        "selected fallback and identity"
                    );
                    let replacement = empty(&mut state, 0);
                    assert!(
                        replacement.worktree > owner.worktree,
                        "selected fallback and identity"
                    );
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Err("worktree_remove_missing"),
                        "selected fallback and identity"
                    );
                    assert!(
                        state.contains(replacement),
                        "selected fallback and identity"
                    );
                }),
            ),
            (
                "changed_snapshot_is_atomic",
                (|_scenario: &str| {
                    let mut state = State::default();
                    let owner = empty(&mut state, 0);
                    let expected = state.removal(owner).unwrap();
                    state.worktrees.get_mut(&owner.worktree).unwrap().path =
                        "/preview/changed".into();
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Err("worktree_remove_changed"),
                        "changed snapshot is atomic"
                    );
                    let expected = state.removal(owner).unwrap();
                    state.projects.get_mut(&0).unwrap().path = "/preview/new-project".into();
                    assert_eq!(
                        state.remove_worktree(&expected),
                        Err("worktree_remove_changed"),
                        "changed snapshot is atomic"
                    );
                    assert!(state.contains(owner), "changed snapshot is atomic");
                    assert_eq!(state.owner(0).worktree, 0, "changed snapshot is atomic");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
