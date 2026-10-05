//! Trash behavior follows harbor-file-service 727ce0a worktree.rs (Apache-2.0).
//! The native library accepts paths, not retained directory handles. Recheck the
//! shared capability traversal before and after the call; this is not isolation
//! from external writers. Never fall back to permanent deletion.
use super::{io_error, path};
use sailry_protocol::{ErrorCode, Fault};
use std::{io::ErrorKind, path::Path};

#[cfg(windows)]
#[path = "trash/windows.rs"]
mod windows;

pub(crate) fn trash(root: &Path, relative: &str) -> Result<(), Fault> {
    relocate(root, relative, native)
}

pub(crate) fn remove_verified(
    root: &Path,
    relative: &str,
    revision: &str,
    stamp: &str,
) -> Result<(), Fault> {
    checked(root, relative, revision, stamp, native)
}

fn checked(
    root: &Path,
    relative: &str,
    revision: &str,
    stamp: &str,
    operation: impl FnOnce(&Path) -> Result<(), Fault>,
) -> Result<(), Fault> {
    if blake3::Hash::from_hex(revision).is_err() || blake3::Hash::from_hex(stamp).is_err() {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid file source revision",
        ));
    }
    relocate(root, relative, |target| {
        super::version::verify(root, relative, revision, stamp)?;
        operation(target)
    })
}

#[cfg(windows)]
use windows::trash as native;

#[cfg(not(windows))]
fn native(target: &Path) -> Result<(), Fault> {
    let context = trash::TrashContext::new();
    #[cfg(target_os = "macos")]
    let context = {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut context = context;
        // A headless Node must not request Finder Automation permission.
        // Recovery remains available by moving the item out of Trash.
        context.set_delete_method(DeleteMethod::NsFileManager);
        context
    };
    context.delete(target).map_err(|_| unknown())
}

fn relocate(
    root: &Path,
    relative: &str,
    operation: impl FnOnce(&Path) -> Result<(), Fault>,
) -> Result<(), Fault> {
    let parts = path::entry_components(relative)?;
    let (name, parents) = parts.split_last().unwrap();
    let dir = path::descend(path::root(root)?, parents)?;
    let parent_id = path::identity::directory(&dir)?;
    dir.symlink_metadata(name).map_err(io_error)?;
    let validate = || {
        let current = path::descend(path::root(root)?, parents)?;
        if path::identity::directory(&current)? == parent_id {
            Ok(())
        } else {
            Err(Fault::new(
                ErrorCode::RevisionConflict,
                "entry parent changed",
            ))
        }
    };
    validate()?;
    // trash-rs canonicalizes the parent only, preserving a symlink leaf.
    operation(&root.join(relative))?;
    validate().map_err(|_| unknown())?;
    match dir.symlink_metadata(name) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        _ => Err(unknown()),
    }
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "move to Trash could not be confirmed; inspect the entry and Trash before retrying",
    )
}

#[cfg(test)]
mod tests;
