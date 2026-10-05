//! Adapted from sailry-code 67ae9fa0 service.rs remove_worktree (Apache-2.0).
use super::super::{check_root, fault, io_error, open, path, revision};
use git2::{
    Branch, Oid, RepositoryState, StatusOptions, Worktree, WorktreeLockStatus, WorktreePruneOptions,
};
use sailry_protocol::{ErrorCode, Fault};
use std::path::Path;

pub(crate) fn remove(root: &Path, expected_head: &str, expected_branch: &str) -> Result<(), Fault> {
    if expected_head.len() != 40
        || expected_branch.len() > 1024
        || !Branch::name_is_valid(expected_branch).map_err(fault)?
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid worktree revision",
        ));
    }
    let expected = Oid::from_str(expected_head).map_err(fault)?;
    let retained = path::root(root)?;
    let repository = open(root)?;
    if !repository.is_worktree() {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "cannot remove the main worktree",
        ));
    }
    let worktree = Worktree::open_from_repository(&repository).map_err(fault)?;
    worktree.validate().map_err(fault)?;
    if worktree.path().canonicalize().map_err(io_error)? != root
        || repository
            .commondir()
            .canonicalize()
            .map_err(io_error)?
            .starts_with(root)
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "worktree registration changed",
        ));
    }
    if matches!(
        worktree.is_locked().map_err(fault)?,
        WorktreeLockStatus::Locked(_)
    ) {
        return Err(Fault::new(ErrorCode::Busy, "worktree is locked"));
    }
    // Git may know a nested worktree whose Node registration was interrupted.
    for name in repository.worktrees().map_err(fault)?.iter() {
        let name = name
            .map_err(fault)?
            .ok_or_else(|| Fault::new(ErrorCode::InvalidRequest, "worktree name is not UTF-8"))?;
        let other = repository.find_worktree(name).map_err(fault)?;
        let other = other
            .path()
            .canonicalize()
            .unwrap_or_else(|_| other.path().to_owned());
        if other != root && other.starts_with(root) {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "worktree contains another Git worktree",
            ));
        }
    }
    if repository.state() != RepositoryState::Clean {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "finish the current Git operation before removing the worktree",
        ));
    }
    let head = repository.find_reference("HEAD").map_err(fault)?;
    let reference = format!("refs/heads/{expected_branch}");
    if head.symbolic_target().map_err(fault)? != Some(reference.as_str())
        || head.resolve().map_err(fault)?.target() != Some(expected)
    {
        return Err(revision::conflict());
    }
    let mut status = StatusOptions::new();
    status.include_untracked(true).recurse_untracked_dirs(true);
    if !repository
        .statuses(Some(&mut status))
        .map_err(fault)?
        .is_empty()
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "worktree has uncommitted changes",
        ));
    }
    if repository
        .path()
        .join("index.lock")
        .try_exists()
        .map_err(io_error)?
        || repository
            .path()
            .join("HEAD.lock")
            .try_exists()
            .map_err(io_error)?
    {
        return Err(Fault::new(
            ErrorCode::Busy,
            "worktree has an active Git lock",
        ));
    }
    let mut options = WorktreePruneOptions::new();
    options.valid(true).working_tree(true);
    check_root(&retained, root)?;
    // Release our native repository handles before recursive removal, including on Windows.
    drop(head);
    drop(repository);
    drop(retained);
    worktree.prune(Some(&mut options)).map_err(|_| unknown())?;
    if root.try_exists().map_err(|_| unknown())? {
        return Err(unknown());
    }
    Ok(())
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "worktree removal may be partial; inspect the directory and Git registration before retrying",
    )
}
