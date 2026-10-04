//! Worktree management retains the mounted package's captured source checkout.
use super::*;
use sailry_protocol::{ProjectId, WorktreeId};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Change {
    Register {
        project: ProjectId,
        path: String,
    },
    Create {
        project: ProjectId,
        path: String,
        branch: String,
        commit: String,
    },
    Managed {
        project: ProjectId,
        branch: String,
        expected_head: String,
        expected_index: String,
        include_changes: bool,
    },
    Remove {
        worktree: WorktreeId,
        expected_head: String,
        expected_branch: String,
    },
}

impl Change {
    fn command(self, source: WorktreeId) -> Command {
        match self {
            Self::Register { project, path } => Command::RegisterWorktree { project, path },
            Self::Create {
                project,
                path,
                branch,
                commit,
            } => Command::CreateWorktree {
                project,
                path,
                branch,
                commit,
            },
            Self::Managed {
                project,
                branch,
                expected_head,
                expected_index,
                include_changes,
            } => Command::CreateManagedWorktree {
                project,
                source,
                branch,
                expected_head,
                expected_index,
                include_changes,
            },
            Self::Remove {
                worktree,
                expected_head,
                expected_branch,
            } => Command::RemoveWorktree {
                worktree,
                expected_head,
                expected_branch,
            },
        }
    }
}

impl Host {
    pub(super) fn worktree_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let catalog = self.clone();
        let list = self.clone();
        let change = self.clone();
        module
            .async_function("readWorktreeCatalog", move |_| {
                catalog.read_scoped(
                    Command::ReadWorktreeCatalog {
                        worktree: catalog.worktree()?,
                    },
                    true,
                )
            })
            .async_function("listWorktrees", move |_| {
                list.read_scoped(
                    Command::ListWorktrees {
                        worktree: list.worktree()?,
                    },
                    true,
                )
            })
            .function("prepareWorktreeChange", move |args| {
                let draft: Change = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                change.prepare_public(draft.command(change.worktree()?))
            })
    }

    fn worktree(&self) -> Result<WorktreeId, HostError> {
        self.check()?;
        self.context
            .worktree
            .ok_or_else(|| HostError::new("plugin has no worktree scope"))
    }
}
