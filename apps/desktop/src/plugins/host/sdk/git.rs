//! Git writes use the captured checkout and the ordinary Node admission ledger.
mod change;
use super::*;
use sailry_protocol::{GitCommitOptions, GitIndexChange, WorktreeId};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexDraft {
    operation: GitIndexChange,
    paths: Vec<String>,
    expected_index: String,
    expected_head: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitDraft {
    message: String,
    #[serde(default)]
    amend: bool,
    #[serde(default)]
    options: GitCommitOptions,
    expected_index: String,
    expected_head: Option<String>,
    expected_branch: Option<String>,
}

impl Host {
    pub(super) fn git_module(self: &Arc<Self>, module: HostModule) -> HostModule {
        let inspect = self.clone();
        let index = self.clone();
        let commit = self.clone();
        let branches = self.clone();
        let resolve = self.clone();
        let diff = self.clone();
        let log = self.clone();
        let history = self.clone();
        let stash = self.clone();
        let output = self.clone();
        let tags = self.clone();
        let change = self.clone();
        module
            .async_function("inspectGit", move |_| {
                inspect.read_public(Command::InspectGit {
                    worktree: inspect.git_worktree()?,
                })
            })
            .async_function("listGitBranches", move |_| {
                branches.read_scoped(
                    Command::ListGitBranches {
                        worktree: branches.git_worktree()?,
                    },
                    true,
                )
            })
            .async_function("resolveGitRevision", move |args| {
                resolve.read_scoped(
                    Command::ResolveGitRevision {
                        worktree: resolve.git_worktree()?,
                        revision: args.string(0)?.into(),
                    },
                    true,
                )
            })
            .async_function("readGitDiff", move |args| {
                let scope = serde_json::from_value(decode(args.value(1)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                diff.read_scoped(
                    Command::ReadGitDiff {
                        worktree: diff.git_worktree()?,
                        path: args.string(0)?.into(),
                        scope,
                    },
                    true,
                )
            })
            .async_function("readGitLog", move |args| {
                let limit = args
                    .integer(0)?
                    .try_into()
                    .map_err(|_| HostError::new("invalid Git history page limit"))?;
                let cursor = serde_json::from_value(
                    args.get(1).map(decode).transpose()?.unwrap_or(Value::Null),
                )
                .map_err(|error| HostError::new(error.to_string()))?;
                log.read_scoped(
                    Command::ReadGitLog {
                        worktree: log.git_worktree()?,
                        limit,
                        cursor,
                    },
                    true,
                )
            })
            .async_function("readGitCommit", move |args| {
                history.read_scoped(
                    Command::ReadGitCommit {
                        worktree: history.git_worktree()?,
                        commit: args.string(0)?.into(),
                    },
                    true,
                )
            })
            .async_function("readGitStash", move |args| {
                stash.read_scoped(
                    Command::ReadGitStash {
                        worktree: stash.git_worktree()?,
                        commit: args.string(0)?.into(),
                    },
                    true,
                )
            })
            .async_function("readGitOutput", move |_| {
                output.read_scoped(
                    Command::ReadGitOutput {
                        worktree: output.git_worktree()?,
                    },
                    true,
                )
            })
            .async_function("listGitRemoteTags", move |args| {
                tags.read_scoped(
                    Command::ListGitRemoteTags {
                        worktree: tags.git_worktree()?,
                        remote: args.string(0)?.into(),
                    },
                    true,
                )
            })
            .function("prepareGitChange", move |args| {
                let draft: change::Change = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                change.prepare_public(draft.command(change.git_worktree()?))
            })
            .function("prepareGitIndex", move |args| {
                let draft: IndexDraft = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                index.prepare_public(Command::UpdateGitIndex {
                    worktree: index.git_worktree()?,
                    operation: draft.operation,
                    paths: draft.paths,
                    expected_index: draft.expected_index,
                    expected_head: draft.expected_head,
                })
            })
            .function("prepareGitCommit", move |args| {
                let draft: CommitDraft = serde_json::from_value(decode(args.value(0)?)?)
                    .map_err(|error| HostError::new(error.to_string()))?;
                commit.prepare_public(Command::CreateGitCommit {
                    worktree: commit.git_worktree()?,
                    message: draft.message,
                    amend: draft.amend,
                    options: draft.options,
                    expected_index: draft.expected_index,
                    expected_head: draft.expected_head,
                    expected_branch: draft.expected_branch,
                })
            })
    }

    fn git_worktree(&self) -> Result<WorktreeId, HostError> {
        self.check()?;
        self.context
            .worktree
            .ok_or_else(|| HostError::new("plugin has no worktree scope"))
    }
}
