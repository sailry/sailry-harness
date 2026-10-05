//! Adapted from sailry-code 67ae9fa0 sailry-git service.rs create_worktree (Apache-2.0).
use super::super::{check_root, fault, io_error, open, path};
use git2::{Branch, BranchType, Oid, WorktreeAddOptions};
use sailry_protocol::{ErrorCode, Fault};
use std::path::Path;

pub(crate) fn create(
    root: &Path,
    destination: &str,
    name: &str,
    commit: &str,
    registration: &str,
) -> Result<String, Fault> {
    if name.len() > 1024 || !Branch::name_is_valid(name).map_err(fault)? {
        return Err(invalid("invalid branch name"));
    }
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("expected a full commit identifier"));
    }
    if destination.len() > 4096 || destination.chars().any(char::is_control) {
        return Err(invalid("invalid worktree destination"));
    }
    let destination = Path::new(destination);
    if !destination.is_absolute() {
        return Err(invalid("worktree destination must be absolute"));
    }
    let filename = destination
        .file_name()
        .ok_or_else(|| invalid("expected a new directory name"))?;
    let parent = destination
        .parent()
        .ok_or_else(|| invalid("expected a destination parent"))?
        .canonicalize()
        .map_err(io_error)?;
    let destination = parent.join(filename);
    let retained_parent = path::root(&parent)?;
    match retained_parent.symlink_metadata(filename) {
        Ok(_) => {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "worktree destination already exists",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    let retained = path::root(root)?;
    let repository = open(root)?;
    let common = repository.commondir().canonicalize().map_err(io_error)?;
    if destination.starts_with(&common) {
        return Err(invalid(
            "worktree destination cannot be inside Git metadata",
        ));
    }
    let base = repository
        .find_commit(Oid::from_str(commit).map_err(fault)?)
        .map_err(fault)?;
    match repository.find_branch(name, BranchType::Local) {
        Ok(_) => return Err(Fault::new(ErrorCode::Conflict, "branch already exists")),
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(fault(error)),
    }
    check_root(&retained, root)?;
    check_root(&retained_parent, &parent)?;
    let branch = repository.branch(name, &base, false).map_err(|error| {
        if error.code() == git2::ErrorCode::Exists {
            Fault::new(ErrorCode::Conflict, "branch already exists")
        } else {
            fault(error)
        }
    })?;
    // From this point, an error may leave a branch, Git metadata or checkout.
    // Preserve those effects for inspection rather than rolling back external work.
    let reference = branch.into_reference();
    repository
        .worktree(
            registration,
            &destination,
            Some(WorktreeAddOptions::new().reference(Some(&reference))),
        )
        .map_err(|_| unknown())?;
    check_root(&retained, root).map_err(|_| unknown())?;
    check_root(&retained_parent, &parent).map_err(|_| unknown())?;
    let created = open(&destination).map_err(|_| unknown())?;
    if created.head().map_err(|_| unknown())?.target() != Some(base.id()) {
        return Err(unknown());
    }
    destination
        .into_os_string()
        .into_string()
        .map_err(|_| unknown())
}

fn invalid(message: &'static str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "worktree creation may have changed the repository or destination; inspect them before retrying",
    )
}
