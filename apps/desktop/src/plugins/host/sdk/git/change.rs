use sailry_protocol::{Command, GitAction, WorktreeId};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Change {
    Create {
        name: String,
        commit: String,
    },
    Rename {
        name: String,
        new_name: String,
        commit: String,
    },
    Delete {
        name: String,
        commit: String,
        expected_head: String,
        expected_branch: Option<String>,
    },
    Switch {
        name: String,
        commit: String,
        expected_head: Option<String>,
        expected_branch: Option<String>,
        expected_index: String,
    },
    Merge {
        name: String,
        commit: String,
        expected_head: String,
        expected_branch: String,
        expected_index: String,
    },
    Action {
        action: GitAction,
        expected_head: Option<String>,
        expected_branch: Option<String>,
        expected_index: String,
    },
}

impl Change {
    pub(super) fn command(self, worktree: WorktreeId) -> Command {
        match self {
            Self::Create { name, commit } => Command::CreateGitBranch {
                worktree,
                name,
                commit,
            },
            Self::Rename {
                name,
                new_name,
                commit,
            } => Command::RenameGitBranch {
                worktree,
                name,
                new_name,
                commit,
            },
            Self::Delete {
                name,
                commit,
                expected_head,
                expected_branch,
            } => Command::DeleteGitBranch {
                worktree,
                name,
                commit,
                expected_head,
                expected_branch,
            },
            Self::Switch {
                name,
                commit,
                expected_head,
                expected_branch,
                expected_index,
            } => Command::SwitchGitBranch {
                worktree,
                name,
                commit,
                expected_head,
                expected_branch,
                expected_index,
            },
            Self::Merge {
                name,
                commit,
                expected_head,
                expected_branch,
                expected_index,
            } => Command::MergeGitBranch {
                worktree,
                name,
                commit,
                expected_head,
                expected_branch,
                expected_index,
            },
            Self::Action {
                action,
                expected_head,
                expected_branch,
                expected_index,
            } => Command::RunGitAction {
                worktree,
                action,
                expected_head,
                expected_branch,
                expected_index,
            },
        }
    }
}
