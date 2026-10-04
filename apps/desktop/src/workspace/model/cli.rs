use super::{Key, Owner, State};
use crate::tr;
use gpui_kit::SharedString;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Profile {
    pub id: &'static str,
    pub name: SharedString,
    pub executable: SharedString,
    pub available: bool,
}

pub(super) fn fixtures() -> BTreeMap<usize, Vec<Profile>> {
    (0..2)
        .map(|host| {
            let profiles = ["codex", "claude", "agy", "grok", "opencode", "kimi"]
                .into_iter()
                .enumerate()
                .map(|(index, id)| Profile {
                    id,
                    name: tr(&format!("cli_{id}")),
                    executable: id.into(),
                    available: if host == 0 { index < 2 } else { index >= 4 },
                })
                .collect();
            (host, profiles)
        })
        .collect()
}

impl State {
    pub fn create_cli_terminal(
        &mut self,
        owner: Owner,
        path: &str,
        profile: &Profile,
    ) -> Result<Key, &'static str> {
        if !self.contains(owner) || self.worktrees[&owner.worktree].path != path {
            return Err("cli_target_changed");
        }
        if !self.projects[&owner.project].trusted {
            return Err("cli_untrusted");
        }
        if !profile.available
            || !self
                .cli_profiles
                .get(&owner.host)
                .is_some_and(|profiles| profiles.iter().any(|current| current == profile))
        {
            return Err("cli_unavailable");
        }
        let key = self.create_terminal(owner);
        let terminal = self.terminals.get_mut(&key).unwrap();
        terminal.title = profile.name.clone();
        terminal.profile = Some(profile.clone());
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_targets() {
        let mut state = State::default();
        let profile = state.cli_profiles[&0][0].clone();
        assert_eq!(
            state.create_cli_terminal(state.owner(1), "/preview/remote-workspace", &profile),
            Err("cli_unavailable")
        );
        let owner = Owner {
            host: 0,
            project: 1,
            worktree: 1,
        };
        assert_eq!(
            state.create_cli_terminal(owner, "/preview/remote-workspace", &profile),
            Err("cli_target_changed")
        );
        assert_eq!(
            state.create_cli_terminal(
                Owner {
                    worktree: 99,
                    ..owner
                },
                "",
                &profile
            ),
            Err("cli_target_changed")
        );
        assert_eq!(state.terminals.len(), 2);
    }

    #[test]
    fn binds_launch_identity() {
        let mut state = State::default();
        let profile = state.cli_profiles[&0][0].clone();
        let owner = Owner {
            worktree: 2,
            ..state.owner(0)
        };
        let path = state.worktrees[&2].path.clone();
        let first = state.create_cli_terminal(owner, &path, &profile).unwrap();
        state.cli_profiles.get_mut(&0).unwrap()[0].executable = "changed".into();
        assert_eq!(state.terminals[&first].profile.as_ref(), Some(&profile));
        assert_eq!(state.terminals[&first].owner, owner);
        assert_eq!(
            state.create_cli_terminal(owner, &path, &profile),
            Err("cli_unavailable")
        );
        state.terminals.remove(&first);
        let next = state.create_terminal(owner);
        assert!(next.1 > first.1);
        assert!(state.terminals[&next].profile.is_none());
    }
}
