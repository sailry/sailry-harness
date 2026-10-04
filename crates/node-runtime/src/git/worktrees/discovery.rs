//! Discovery follows sailry-code 67ae9fa0 command_support.rs inspect_worktrees (Apache-2.0).
use super::super::{Control, check_root, git_error, io_error, open, path};
use git2::{Repository, Worktree, WorktreeLockStatus};
use sailry_protocol::{
    ErrorCode, Fault, GitWorktree, GitWorktrees, MAX_GIT_WORKTREES, RepositoryKind,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub(in crate::git) fn list(root: &Path, control: &Control) -> Result<GitWorktrees, Fault> {
    let repository = match open(root) {
        Ok(repository) => repository,
        Err(error) if error.code == ErrorCode::NotFound && !root.join(".git").exists() => {
            return Ok(GitWorktrees {
                kind: RepositoryKind::Directory,
                entries: vec![],
                truncated: false,
                omitted_paths: 0,
            });
        }
        Err(error) => return Err(error),
    };
    let mut result = GitWorktrees {
        kind: match repository.head() {
            Ok(_) => RepositoryKind::Ready,
            Err(error) if error.code() == git2::ErrorCode::UnbornBranch => RepositoryKind::Unborn,
            Err(error) => return Err(git_error(error)),
        },
        entries: Vec::new(),
        truncated: false,
        omitted_paths: 0,
    };
    let common = repository.commondir().canonicalize().map_err(io_error)?;
    let main = Repository::open(&common).map_err(git_error)?;
    let mut paths = Vec::new();
    if let Some(root) = main.workdir() {
        paths.push((root.to_owned(), true, false));
    }
    for name in repository.worktrees().map_err(git_error)?.iter() {
        control.check()?;
        if paths.len() >= MAX_GIT_WORKTREES {
            result.truncated = true;
            break;
        }
        let name = match name {
            Ok(Some(name)) => name,
            _ => {
                result.omitted_paths += 1;
                continue;
            }
        };
        match repository.find_worktree(name) {
            Ok(worktree) => paths.push((
                worktree.path().to_owned(),
                false,
                matches!(
                    worktree.is_locked().map_err(git_error)?,
                    WorktreeLockStatus::Locked(_)
                ),
            )),
            Err(_) => result.omitted_paths += 1,
        }
    }
    let mut seen = HashSet::new();
    for (path, main, locked) in paths {
        control.check()?;
        let path = path.canonicalize().unwrap_or(path);
        if !seen.insert(path.clone()) {
            continue;
        }
        let Some(value) = path
            .to_str()
            .filter(|value| value.len() <= 4096 && !value.chars().any(char::is_control))
        else {
            result.omitted_paths += 1;
            continue;
        };
        let mut entry = GitWorktree {
            path: value.into(),
            branch: None,
            head: None,
            main,
            locked,
            available: false,
        };
        if let Ok(repository) = member(&common, &path) {
            entry.available = true;
            entry.head = repository
                .head()
                .ok()
                .and_then(|head| head.target())
                .map(|head| head.to_string());
            entry.branch = repository.find_reference("HEAD").ok().and_then(|head| {
                head.symbolic_target()
                    .ok()
                    .flatten()
                    .and_then(|name| name.strip_prefix("refs/heads/"))
                    .map(str::to_owned)
            });
            if entry.branch.as_ref().is_some_and(|name| name.len() > 1024) {
                entry.branch = None;
                result.omitted_paths += 1;
            }
        }
        result.entries.push(entry);
    }
    result
        .entries
        .sort_by(|left, right| right.main.cmp(&left.main).then(left.path.cmp(&right.path)));
    Ok(result)
}

pub(crate) fn resolve(root: &Path, destination: &str) -> Result<String, Fault> {
    if destination.len() > 4096
        || destination.chars().any(char::is_control)
        || !Path::new(destination).is_absolute()
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "expected an absolute worktree directory",
        ));
    }
    let retained = path::root(root)?;
    let repository = open(root)?;
    let common = repository.commondir().canonicalize().map_err(io_error)?;
    let destination = PathBuf::from(destination)
        .canonicalize()
        .map_err(io_error)?;
    let retained_destination = path::root(&destination)?;
    member(&common, &destination)?;
    check_root(&retained, root)?;
    check_root(&retained_destination, &destination)?;
    destination
        .into_os_string()
        .into_string()
        .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "worktree path is not UTF-8"))
}

fn member(common: &Path, root: &Path) -> Result<Repository, Fault> {
    let repository = open(root)?;
    if repository.commondir().canonicalize().map_err(io_error)? != common {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "worktree belongs to a different Git repository",
        ));
    }
    if repository.is_worktree() {
        let worktree = Worktree::open_from_repository(&repository).map_err(git_error)?;
        worktree.validate().map_err(git_error)?;
        if worktree.path().canonicalize().map_err(io_error)? != root {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "worktree registration points to a different directory",
            ));
        }
    }
    Ok(repository)
}
