use super::{Project, State, Worktree};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectRemoval {
    pub id: usize,
    pub project: Project,
    worktrees: BTreeMap<usize, Worktree>,
}

impl State {
    pub fn project_removal(&self, id: usize) -> Option<ProjectRemoval> {
        Some(ProjectRemoval {
            id,
            project: self.projects.get(&id)?.clone(),
            worktrees: self
                .worktrees
                .iter()
                .filter(|(_, tree)| tree.project == id)
                .map(|(&id, tree)| (id, tree.clone()))
                .collect(),
        })
    }

    pub fn project_removal_reason(&self, id: usize) -> Option<&'static str> {
        if !self.projects.contains_key(&id) {
            Some("project_missing")
        } else if self.sessions.values().any(|s| s.owner.project == id) {
            Some("project_remove_sessions")
        } else if self.terminals.values().any(|t| t.owner.project == id) {
            Some("project_remove_terminals")
        } else {
            None
        }
    }

    pub fn remove_project(&mut self, expected: &ProjectRemoval) -> Result<(), &'static str> {
        let current = self.project_removal(expected.id).ok_or("project_missing")?;
        if current != *expected {
            return Err("project_changed");
        }
        if let Some(reason) = self.project_removal_reason(expected.id) {
            return Err(reason);
        }
        // Metadata-only preview: never remove directories or execute Git commands.
        self.projects.remove(&expected.id);
        self.worktrees.retain(|_, tree| tree.project != expected.id);
        let host = expected.project.host;
        if self.selected.get(&host) == Some(&expected.id) {
            self.selected.remove(&host);
            if let Some((&id, _)) = self.projects.iter().find(|(_, p)| p.host == host) {
                self.selected.insert(host, id);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty(state: &mut State, host: usize) -> usize {
        state
            .save_project(host, None, "Temporary", "/preview/temporary")
            .unwrap()
    }

    #[test]
    fn blocks_owned_resources() {
        let mut state = State::default();
        let id = empty(&mut state, 0);
        let owner = state.project_owner(id).unwrap();
        let expected = state.project_removal(id).unwrap();
        let key = state.create_session(owner);
        state.sessions.get_mut(&key).unwrap().archived = true;
        assert_eq!(
            state.remove_project(&expected),
            Err("project_remove_sessions")
        );
        state.sessions.remove(&key);
        let key = state.create_terminal(owner);
        assert_eq!(
            state.remove_project(&expected),
            Err("project_remove_terminals")
        );
        state.terminals.remove(&key);
        assert_eq!(state.project_removal(id), Some(expected.clone()));
        assert_eq!(state.remove_project(&expected), Ok(()));
    }

    #[test]
    fn stale_snapshot_is_atomic() {
        let mut state = State::default();
        let id = empty(&mut state, 0);
        let expected = state.project_removal(id).unwrap();
        state.projects.get_mut(&id).unwrap().path = "/preview/changed".into();
        assert_eq!(state.remove_project(&expected), Err("project_changed"));
        state.projects.get_mut(&id).unwrap().repository = super::super::Repository::Ready;
        let expected = state.project_removal(id).unwrap();
        state
            .add_worktree(id, "main", "feature/new", "/preview/new")
            .unwrap();
        assert_eq!(state.remove_project(&expected), Err("project_changed"));
        assert!(state.projects.contains_key(&id));
        assert_eq!(
            state
                .worktrees
                .values()
                .filter(|tree| tree.project == id)
                .count(),
            2
        );
    }

    #[test]
    fn scopes_selection_fallback() {
        let mut state = State::default();
        let id = empty(&mut state, 1);
        state.select(state.project_owner(id).unwrap());
        let local = state.owner(0);
        let expected = state.project_removal(id).unwrap();
        state.remove_project(&expected).unwrap();
        assert_eq!(state.owner(1).project, 1);
        assert_eq!(state.owner(0), local);
        assert_eq!(state.remove_project(&expected), Err("project_missing"));
        assert!(empty(&mut state, 1) > id);
    }

    #[test]
    fn retains_empty_host() {
        let mut state = State::default();
        state.sessions.retain(|key, _| key.0 != 1);
        state.terminals.retain(|key, _| key.0 != 1);
        let expected = state.project_removal(1).unwrap();
        state.remove_project(&expected).unwrap();
        assert_eq!(state.selected_owner(1), None);
        assert!(state.worktrees.values().all(|tree| tree.project != 1));
        let id = empty(&mut state, 1);
        let owner = state.project_owner(id).unwrap();
        assert!(state.select(owner));
        assert_eq!(state.owner(1), owner);
        assert!(
            state
                .save_project(2, None, "Unknown", "/preview/unknown")
                .is_err()
        );
    }
}
