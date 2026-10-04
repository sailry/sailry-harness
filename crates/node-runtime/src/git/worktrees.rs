//! Ownership traversal follows sailry-code 67ae9fa0 command_support.rs (Apache-2.0).
use super::{git_error, io_error};
use git2::{Repository, RepositoryState};
use sailry_protocol::{ErrorCode, Fault};
use std::path::Path;

mod managed;
pub(crate) use managed::create as managed;
mod creation;
pub(crate) use creation::create;
mod discovery;
pub(super) use discovery::list;
pub(crate) use discovery::resolve;
mod removal;
pub(crate) use removal::remove;
mod pruning;
pub(super) use pruning::prune;

pub(super) fn check_available(
    repository: &Repository,
    root: &Path,
    reference: &str,
) -> Result<(), Fault> {
    visit(repository, |repository| {
        let Some(workdir) = repository.workdir() else {
            return Ok(());
        };
        if workdir.canonicalize().map_err(io_error)? != root && owns(repository, reference)? {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "branch is checked out in another worktree",
            ));
        }
        Ok(())
    })
}

pub(super) fn check_mutable(repository: &Repository, reference: &str) -> Result<(), Fault> {
    visit(repository, |repository| {
        if owns(repository, reference)? && repository.state() != RepositoryState::Clean {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "finish the current Git operation before renaming its branch",
            ));
        }
        Ok(())
    })
}

fn owns(repository: &Repository, reference: &str) -> Result<bool, Fault> {
    Ok(repository
        .find_reference("HEAD")
        .map_err(git_error)?
        .symbolic_target()
        .map_err(git_error)?
        == Some(reference))
}

fn visit(
    repository: &Repository,
    mut check: impl FnMut(&Repository) -> Result<(), Fault>,
) -> Result<(), Fault> {
    check(&Repository::open(repository.commondir()).map_err(git_error)?)?;
    for name in repository.worktrees().map_err(git_error)?.iter() {
        let name = name
            .map_err(git_error)?
            .ok_or_else(|| Fault::new(ErrorCode::InvalidRequest, "worktree name is not UTF-8"))?;
        let worktree = repository.find_worktree(name).map_err(git_error)?;
        check(&Repository::open(worktree.path()).map_err(git_error)?)?;
    }
    Ok(())
}
