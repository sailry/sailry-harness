//! Pruning drops one stale Git registration, never files or branch references.
use super::{fault, io_error};
use git2::{Repository, WorktreeLockStatus, WorktreePruneOptions};
use sailry_protocol::{ErrorCode, Fault};
use std::{io::ErrorKind, path::Path};

pub(in crate::git) fn prune(repository: &Repository, path: &Path) -> Result<(), Fault> {
    if !path.is_absolute() {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "worktree path must be absolute",
        ));
    }
    for name in repository.worktrees().map_err(fault)?.iter() {
        let name = name
            .map_err(fault)?
            .ok_or_else(|| Fault::new(ErrorCode::InvalidRequest, "worktree name is not UTF-8"))?;
        let worktree = repository.find_worktree(name).map_err(fault)?;
        if worktree.path() != path {
            continue;
        }
        if matches!(
            worktree.is_locked().map_err(fault)?,
            WorktreeLockStatus::Locked(_)
        ) {
            return Err(Fault::new(ErrorCode::Busy, "worktree is locked"));
        }
        // Unlike exists(), metadata distinguishes inaccessible paths from missing ones
        // and preserves dangling symlinks as existing user-owned filesystem entries.
        match std::fs::symlink_metadata(path) {
            Ok(_) => {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "worktree path still exists",
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        let mut options = WorktreePruneOptions::new();
        options.valid(false).locked(false).working_tree(false);
        if !worktree.is_prunable(Some(&mut options)).map_err(fault)? {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "worktree registration is not stale",
            ));
        }
        return worktree.prune(Some(&mut options)).map_err(|_| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "worktree registration cleanup may be incomplete",
            )
        });
    }
    Err(Fault::new(
        ErrorCode::NotFound,
        "linked worktree registration not found",
    ))
}
