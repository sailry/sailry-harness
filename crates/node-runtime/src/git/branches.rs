//! Branch reads and creation adapted from sailry-code 67ae9fa0:
//! sailry-git/src/service.rs branches/create_branch (Apache-2.0).
use super::{Control, check_root, git_error, open, path};
use git2::{Branch, BranchType, Oid};
use sailry_protocol::*;
use std::path::Path;

mod removal;
mod rename;
pub(crate) use removal::remove;
pub(crate) use rename::rename;

pub(super) fn list(root: &Path, control: &Control) -> Result<GitBranches, Fault> {
    let repository = match open(root) {
        Ok(repository) => repository,
        Err(error) if error.code == ErrorCode::NotFound && !root.join(".git").exists() => {
            return Ok(GitBranches {
                kind: RepositoryKind::Directory,
                sync: GitSync::default(),
                current: None,
                entries: vec![],
                truncated: false,
                omitted_names: 0,
            });
        }
        Err(error) => return Err(error),
    };
    let unborn = match repository.head() {
        Ok(_) => false,
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => true,
        Err(error) => return Err(git_error(error)),
    };
    let current = repository
        .find_reference("HEAD")
        .map_err(git_error)?
        .symbolic_target()
        .map_err(git_error)?
        .and_then(|name| name.strip_prefix("refs/heads/"))
        .map(str::to_owned);
    let mut result = GitBranches {
        kind: if unborn {
            RepositoryKind::Unborn
        } else {
            RepositoryKind::Ready
        },
        sync: sync(root, current.as_deref())?,
        current,
        entries: Vec::new(),
        truncated: false,
        omitted_names: 0,
    };
    // Bound inspection as well as output; symbolic remote HEAD aliases are not branches.
    for (index, branch) in repository.branches(None).map_err(git_error)?.enumerate() {
        control.check()?;
        if index == MAX_GIT_ENTRIES {
            result.truncated = true;
            break;
        }
        let (branch, kind) = branch.map_err(git_error)?;
        if branch.get().symbolic_target_bytes().is_some() {
            continue;
        }
        let Some(name) = branch.name().map_err(git_error)? else {
            result.omitted_names += 1;
            continue;
        };
        if name.len() > 1024 || name.chars().any(char::is_control) {
            result.omitted_names += 1;
            continue;
        }
        let Some(commit) = branch.get().target() else {
            result.omitted_names += 1;
            continue;
        };
        let remote = kind == BranchType::Remote;
        result.entries.push(GitBranch {
            name: name.into(),
            commit: commit.to_string(),
            remote,
            current: !remote && result.current.as_deref() == Some(name),
        });
    }
    result
        .entries
        .sort_by(|left, right| (left.remote, &left.name).cmp(&(right.remote, &right.name)));
    Ok(result)
}

pub(crate) fn create(root: &Path, name: &str, commit: &str) -> Result<GitBranch, Fault> {
    if name.len() > 1024 || !Branch::name_is_valid(name).map_err(git_error)? {
        return Err(Fault::new(ErrorCode::InvalidRequest, "invalid branch name"));
    }
    // The caller selects an exact base commit, not a moving reference or revspec.
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "expected a full commit identifier",
        ));
    }
    let retained = path::root(root)?;
    let repository = open(root)?;
    // libgit2 may accept an existing reference already pointing to the same commit.
    // Keep the UI's explicit create operation distinct from request-ID deduplication.
    match repository.find_branch(name, BranchType::Local) {
        Ok(_) => return Err(Fault::new(ErrorCode::Conflict, "branch already exists")),
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(git_error(error)),
    }
    let commit = repository
        .find_commit(Oid::from_str(commit).map_err(git_error)?)
        .map_err(git_error)?;
    check_root(&retained, root)?;
    let branch = repository.branch(name, &commit, false).map_err(|error| {
        if error.code() == git2::ErrorCode::Exists {
            Fault::new(ErrorCode::Conflict, "branch already exists")
        } else {
            git_error(error)
        }
    })?;
    check_root(&retained, root).map_err(|_| {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            "branch creation could not be confirmed",
        )
    })?;
    Ok(GitBranch {
        name: name.into(),
        commit: commit.id().to_string(),
        remote: false,
        current: branch.is_head(),
    })
}

fn sync(root: &Path, branch: Option<&str>) -> Result<GitSync, Fault> {
    let mut repository = git2::Repository::open(root).map_err(git_error)?;
    let mut stashes = Vec::new();
    repository
        .stash_foreach(|_, message, id| {
            stashes.push(GitStash {
                commit: id.to_string(),
                message: message.to_owned(),
            });
            stashes.len() < MAX_GIT_ENTRIES
        })
        .map_err(git_error)?;
    let tags = repository
        .tag_names(None)
        .map_err(git_error)?
        .iter()
        .flatten()
        .flatten()
        .take(MAX_GIT_ENTRIES)
        .filter_map(|name| {
            let reference = repository
                .find_reference(&format!("refs/tags/{name}"))
                .ok()?;
            Some(GitTag {
                name: name.into(),
                commit: reference.target()?.to_string(),
            })
        })
        .collect();
    let remote_urls = repository
        .remotes()
        .map_err(git_error)?
        .iter()
        .flatten()
        .flatten()
        .take(MAX_GIT_ENTRIES)
        .filter_map(|name| {
            let remote = repository.find_remote(name).ok()?;
            Some((
                name.to_owned(),
                super::output::redact(remote.url().unwrap_or_default()),
            ))
        })
        .collect();
    let tracking = repository
        .branches(Some(BranchType::Local))
        .map_err(git_error)?
        .take(MAX_GIT_ENTRIES)
        .filter_map(|branch| {
            let (branch, _) = branch.ok()?;
            let local = branch.name().ok()??.to_owned();
            let remote = branch.upstream().ok()?.name().ok()??.to_owned();
            Some((local, remote))
        })
        .collect();
    let mut sync = GitSync {
        tags,
        remote_urls,
        tracking,
        stashes,
        head_message: repository
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok())
            .map(|commit| {
                String::from_utf8_lossy(commit.message_bytes())
                    .chars()
                    .take(65536)
                    .collect()
            }),
        head_summary: repository
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok())
            .and_then(|commit| commit.summary().ok().flatten().map(str::to_owned)),
        remotes: repository
            .remotes()
            .map_err(git_error)?
            .iter()
            .flatten()
            .flatten()
            .take(MAX_GIT_ENTRIES)
            .map(str::to_owned)
            .collect(),
        operation: match repository.state() {
            git2::RepositoryState::Clean => None,
            git2::RepositoryState::Merge => Some("merge".into()),
            git2::RepositoryState::CherryPick | git2::RepositoryState::CherryPickSequence => {
                Some("cherry-pick".into())
            }
            _ => Some("rebase".into()),
        },
        ..Default::default()
    };
    if let Some(branch) =
        branch.and_then(|name| repository.find_branch(name, BranchType::Local).ok())
        && let Ok(upstream) = branch.upstream()
    {
        sync.upstream = upstream.name().ok().flatten().map(str::to_owned);
        if let (Some(head), Some(remote)) = (branch.get().target(), upstream.get().target()) {
            (sync.ahead, sync.behind) = repository
                .graph_ahead_behind(head, remote)
                .map_err(git_error)?;
        }
    }
    Ok(sync)
}
