//! Registered repository access follows the immutable turn's project.
use super::*;
use crate::agent::tools::decode;
use sailry_protocol::{GitDiffScope, GitLogCursor, Output, Worktree, WorktreeId};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    worktree: Option<WorktreeId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Diff {
    worktree: Option<WorktreeId>,
    path: String,
    scope: GitDiffScope,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    worktree: Option<WorktreeId>,
    limit: usize,
    cursor: Option<GitLogCursor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    branch: String,
    expected_head: String,
    expected_index: String,
    include_changes: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Register {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    worktree: WorktreeId,
    expected_head: String,
    expected_branch: String,
}

pub(super) fn handles(operation: Operation) -> bool {
    matches!(
        operation,
        Operation::InspectGit
            | Operation::ReadGitDiff
            | Operation::ReadGitLog
            | Operation::ListWorktrees
            | Operation::CreateManagedWorktree
            | Operation::RegisterWorktree
            | Operation::RemoveWorktree
    )
}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::InspectGit => "Read Git status in the captured project",
        Operation::ReadGitDiff => "Read a bounded file diff in the captured project",
        Operation::ReadGitLog => "Read commit history in the captured project",
        Operation::ListWorktrees => "List Git and registered worktrees in the captured project",
        Operation::CreateManagedWorktree => "Create a worktree from the captured checkout",
        Operation::RegisterWorktree => "Register a worktree from the captured repository",
        Operation::RemoveWorktree => "Remove an unused worktree from the captured project",
        _ => unreachable!("not a repository operation"),
    }
}

pub(super) fn parameters(operation: Operation) -> Value {
    let target = json!({"type":["string","null"]});
    let string = json!({"type":"string"});
    let (properties, required) = match operation {
        Operation::InspectGit => (json!({"worktree":target}), vec!["worktree"]),
        Operation::ReadGitDiff => (
            json!({"worktree":target,"path":string,"scope":{"type":"string","enum":["all","staged","unstaged"]}}),
            vec!["worktree", "path", "scope"],
        ),
        Operation::ReadGitLog => (
            json!({"worktree":target,"limit":{"type":"integer","minimum":1,"maximum":100},"cursor":{"type":["object","null"],"properties":{"head":string,"offset":{"type":"integer","minimum":0}},"required":["head","offset"],"additionalProperties":false}}),
            vec!["worktree", "limit", "cursor"],
        ),
        Operation::ListWorktrees => (json!({}), vec![]),
        Operation::CreateManagedWorktree => (
            json!({"branch":string,"expected_head":string,"expected_index":string,"include_changes":{"type":"boolean"}}),
            vec![
                "branch",
                "expected_head",
                "expected_index",
                "include_changes",
            ],
        ),
        Operation::RegisterWorktree => (json!({"path":string}), vec!["path"]),
        Operation::RemoveWorktree => (
            json!({"worktree":string,"expected_head":string,"expected_branch":string}),
            vec!["worktree", "expected_head", "expected_branch"],
        ),
        _ => unreachable!("not a repository operation"),
    };
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub(super) fn command(
    operation: Operation,
    arguments: Value,
    captured: WorktreeId,
    registered: &[Worktree],
) -> Result<Command, Fault> {
    let source = registered
        .iter()
        .find(|entry| entry.id == captured)
        .ok_or_else(unavailable)?;
    let target = |id: Option<WorktreeId>| {
        let id = id.unwrap_or(captured);
        registered
            .iter()
            .any(|entry| {
                entry.id == id
                    && (id == captured
                        || source.project.is_some() && entry.project == source.project)
            })
            .then_some(id)
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::PermissionDenied,
                    "worktree is outside this turn's project",
                )
            })
    };
    let project = || {
        source.project.ok_or_else(|| {
            Fault::new(
                ErrorCode::InvalidRequest,
                "worktree management requires a project",
            )
        })
    };
    Ok(match operation {
        Operation::InspectGit => {
            let input: Status = decode(arguments)?;
            Command::InspectGit {
                worktree: target(input.worktree)?,
            }
        }
        Operation::ReadGitDiff => {
            let input: Diff = decode(arguments)?;
            Command::ReadGitDiff {
                worktree: target(input.worktree)?,
                path: input.path,
                scope: input.scope,
            }
        }
        Operation::ReadGitLog => {
            let input: History = decode(arguments)?;
            Command::ReadGitLog {
                worktree: target(input.worktree)?,
                limit: input.limit,
                cursor: input.cursor,
            }
        }
        Operation::ListWorktrees => {
            let _: Empty = decode(arguments)?;
            Command::ListWorktrees { worktree: captured }
        }
        Operation::CreateManagedWorktree => {
            let input: Create = decode(arguments)?;
            Command::CreateManagedWorktree {
                project: project()?,
                source: captured,
                branch: input.branch,
                expected_head: input.expected_head,
                expected_index: input.expected_index,
                include_changes: input.include_changes,
            }
        }
        Operation::RegisterWorktree => {
            let input: Register = decode(arguments)?;
            Command::RegisterWorktree {
                project: project()?,
                path: input.path,
            }
        }
        Operation::RemoveWorktree => {
            let input: Remove = decode(arguments)?;
            Command::RemoveWorktree {
                worktree: target(Some(input.worktree))?,
                expected_head: input.expected_head,
                expected_branch: input.expected_branch,
            }
        }
        _ => unreachable!("not a repository operation"),
    })
}

pub(super) fn output(
    output: Output,
    current: WorktreeId,
    registered: &[Worktree],
) -> Result<Value, Fault> {
    let Output::GitWorktrees(trees) = output else {
        return serde_json::to_value(output).map_err(|_| {
            Fault::new(
                ErrorCode::Internal,
                "repository result could not be encoded",
            )
        });
    };
    let entries: Vec<_> = trees
        .entries
        .iter()
        .map(|tree| {
            let id = registered
                .iter()
                .find(|entry| entry.path == tree.path)
                .map(|entry| entry.id);
            let mut value = serde_json::to_value(tree).expect("worktree is serializable");
            value["id"] = json!(id);
            value["current"] = json!(id == Some(current));
            value
        })
        .collect();
    Ok(
        json!({"kind":"git_worktrees","data":{"current":current,"kind":trees.kind,
        "entries":entries,"truncated":trees.truncated,"omitted_paths":trees.omitted_paths}}),
    )
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::NotFound, "captured worktree is unavailable")
}
