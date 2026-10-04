//! Rename semantics follow harbor-file-service 727ce0a worktree.rs (Apache-2.0).
//! Both parents use the shared retained capability traversal.
use super::{io_error, path};
use path::identity::{self, Identity};
use sailry_protocol::{ErrorCode, Fault};
use std::{io::ErrorKind, path::Path};

pub(crate) fn rename(root: &Path, from: &str, to: &str) -> Result<(), Fault> {
    between(root, root, from, to)
}

pub(crate) fn between(source: &Path, target: &Path, from: &str, to: &str) -> Result<(), Fault> {
    relocate(
        source,
        target,
        from,
        to,
        #[cfg(test)]
        &|_| {},
    )
}

#[cfg(test)]
enum Phase {
    Prepared,
    Moved,
}

fn relocate(
    source_root: &Path,
    target_root: &Path,
    from: &str,
    to: &str,
    #[cfg(test)] hook: &dyn Fn(Phase),
) -> Result<(), Fault> {
    let source = path::entry_components(from)?;
    let target = path::entry_components(to)?;
    let (source_name, source_parents) = source.split_last().unwrap();
    let (target_name, target_parents) = target.split_last().unwrap();
    let source_dir = path::descend(path::root(source_root)?, source_parents)?;
    let target_dir = path::descend(path::root(target_root)?, target_parents)?;
    let source_parent = identity::directory(&source_dir)?;
    let target_parent = identity::directory(&target_dir)?;
    source_dir.symlink_metadata(source_name).map_err(io_error)?;
    if source_name == target_name && source_parent == target_parent {
        return Ok(());
    }
    #[cfg(test)]
    hook(Phase::Prepared);
    validate_parent(source_root, source_parents, source_parent)?;
    validate_parent(target_root, target_parents, target_parent)?;
    match target_dir.symlink_metadata(target_name) {
        Ok(_) => {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "an entry already exists at this path",
            ));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(error)),
    }
    // As with saving, this checks ordinary conflicts; it does not isolate
    // against uncooperative external writers between checking and renaming.
    match source_dir.rename(source_name, &target_dir, target_name) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::CrossesDevices => {
            return super::copy::relocate(source_root, target_root, from, to);
        }
        Err(error) => return Err(io_error(error)),
    }
    #[cfg(test)]
    hook(Phase::Moved);
    let unknown = |_| {
        Fault::new(
            ErrorCode::OutcomeUnknown,
            "entry move could not be confirmed; inspect both locations before retrying",
        )
    };
    validate_parent(source_root, source_parents, source_parent).map_err(unknown)?;
    validate_parent(target_root, target_parents, target_parent).map_err(unknown)?;
    Ok(())
}

fn validate_parent(root: &Path, parts: &[&str], expected: Identity) -> Result<(), Fault> {
    let current = path::descend(path::root(root)?, parts)?;
    if identity::directory(&current)? == expected {
        Ok(())
    } else {
        Err(Fault::new(
            ErrorCode::RevisionConflict,
            "entry parent changed",
        ))
    }
}

#[cfg(test)]
mod tests;
