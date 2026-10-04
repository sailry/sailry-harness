//! Copy/move behavior follows harbor-file-service 727ce0a
//! transfer.rs and worktree.rs (Apache-2.0).
//! Retained capabilities and maintained staging replace ambient recursive copying.
use super::{io_error, open_regular, path};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use cap_tempfile::TempDir;
use path::identity::{self, Identity};
use sailry_protocol::{ErrorCode, Fault};
use std::{io::ErrorKind, path::Path};

#[path = "copy/contents.rs"]
mod contents;
#[path = "copy/evidence.rs"]
mod evidence;
#[cfg(test)]
mod tests;

pub(crate) fn entry(root: &Path, from: &str, to: &str) -> Result<(), Fault> {
    between(root, root, from, to)
}

pub(crate) fn between(source: &Path, target: &Path, from: &str, to: &str) -> Result<(), Fault> {
    publish(
        source,
        target,
        from,
        to,
        None,
        #[cfg(test)]
        &|_| {},
    )
}

pub(crate) fn relocate(source: &Path, target: &Path, from: &str, to: &str) -> Result<(), Fault> {
    publish(
        source,
        target,
        from,
        to,
        Some(&super::trash::trash),
        #[cfg(test)]
        &|_| {},
    )
}

#[cfg(test)]
enum Phase {
    Prepared,
    Published,
    Recycled,
}

type Recycle<'a> = Option<&'a dyn Fn(&Path, &str) -> Result<(), Fault>>;

fn publish(
    source_root: &Path,
    target_root: &Path,
    from: &str,
    to: &str,
    recycle: Recycle<'_>,
    #[cfg(test)] hook: &dyn Fn(Phase),
) -> Result<(), Fault> {
    let source = path::entry_components(from)?;
    let target = path::entry_components(to)?;
    let (source_name, source_parents) = source.split_last().unwrap();
    let (target_name, target_parents) = target.split_last().unwrap();
    let source_dir = path::descend(path::root(source_root)?, source_parents)?;
    let source_parent = identity::directory(&source_dir)?;
    let object = object(&source_dir, source_name)?;
    let mut target_dir = path::root(target_root)?;
    // A different registered root can already be below the source directory.
    // Canonical paths cover native case aliases; retained identities below still
    // check for changes before publishing. This is not a filesystem snapshot.
    if object.directory
        && target_root
            .canonicalize()
            .map_err(io_error)?
            .starts_with(source_root.join(from).canonicalize().map_err(io_error)?)
    {
        return Err(invalid("a directory cannot be copied into itself"));
    }
    for name in target_parents {
        if object.directory && identity::directory(&target_dir)? == object.id {
            return Err(invalid("a directory cannot be copied into itself"));
        }
        target_dir = target_dir.open_dir_nofollow(name).map_err(io_error)?;
        identity::directory(&target_dir)?;
    }
    if object.directory && identity::directory(&target_dir)? == object.id {
        return Err(invalid("a directory cannot be copied into itself"));
    }
    let target_parent = identity::directory(&target_dir)?;
    absent(&target_dir, target_name)?;
    // Keep staging on the target filesystem. The library cleans only its owned
    // temporary directory, not a published target. Cleanup failures are reported.
    // Abrupt process termination can leave staging, as with ordinary temp dirs.
    let staging = TempDir::new_in(&target_dir).map_err(io_error)?;
    let result = (|| {
        #[cfg(unix)]
        {
            use cap_std::fs::PermissionsExt;
            staging
                .set_permissions(".", cap_std::fs::Permissions::from_mode(0o700))
                .map_err(io_error)?;
        }
        let evidence = contents::Copier {
            count: 0,
            moving: recycle.is_some(),
        }
        .copy(&source_dir, source_name, &staging, "entry", 0)?;
        let staged = self::object(&staging, "entry")?;
        #[cfg(test)]
        hook(Phase::Prepared);
        validate_parent(source_root, source_parents, source_parent)?;
        validate_parent(target_root, target_parents, target_parent)?;
        if self::object(&source_dir, source_name)? != object {
            return Err(changed());
        }
        if let Some(evidence) = &evidence {
            evidence.verify(&source_dir, source_name, evidence::Side::Source)?;
            evidence.verify(&staging, "entry", evidence::Side::Target)?;
            validate_parent(source_root, source_parents, source_parent)?;
            validate_parent(target_root, target_parents, target_parent)?;
        }
        absent(&target_dir, target_name)?;
        // Ordinary conflict checks, like rename/save: no isolation from external
        // writers between checking a destination and the filesystem rename.
        staging
            .rename("entry", &target_dir, target_name)
            .map_err(io_error)?;
        #[cfg(test)]
        hook(Phase::Published);
        #[cfg(unix)]
        target_dir
            .try_clone()
            .and_then(|dir| dir.into_std_file().sync_all())
            .map_err(|_| unknown())?;
        validate_parent(target_root, target_parents, target_parent).map_err(|_| unknown())?;
        if self::object(&target_dir, target_name).map_err(|_| unknown())? != staged {
            return Err(unknown());
        }
        if let (Some(evidence), Some(recycle)) = (&evidence, recycle) {
            // A root timestamp cannot detect changes inside descendants. Native
            // Trash is path-based; these checks do not isolate external writers
            // during the final system call.
            evidence
                .verify(&target_dir, target_name, evidence::Side::Target)
                .and_then(|()| evidence.verify(&source_dir, source_name, evidence::Side::Source))
                .and_then(|()| validate_parent(target_root, target_parents, target_parent))
                .and_then(|()| validate_parent(source_root, source_parents, source_parent))
                .and_then(|()| recycle(source_root, from))
                .map_err(|_| move_unknown())?;
            #[cfg(test)]
            hook(Phase::Recycled);
            validate_parent(target_root, target_parents, target_parent)
                .and_then(|()| evidence.verify(&target_dir, target_name, evidence::Side::Target))
                .map_err(|_| move_unknown())?;
        }
        Ok(())
    })();
    match (result, staging.close()) {
        (result, Ok(())) => result,
        (Err(error), Err(_)) if error.code != ErrorCode::OutcomeUnknown => Err(Fault::new(
            ErrorCode::Internal,
            "copy failed; temporary data remains in the destination folder",
        )),
        (_, Err(_)) => Err(Fault::new(
            ErrorCode::OutcomeUnknown,
            "copy cleanup failed; inspect the destination and its temporary data before retrying",
        )),
    }
}

#[derive(PartialEq, Eq)]
struct Object {
    id: Identity,
    directory: bool,
    size: u64,
    modified: Option<cap_std::time::SystemTime>,
    permissions: cap_std::fs::Permissions,
}

fn object(dir: &Dir, name: &str) -> Result<Object, Fault> {
    let metadata = dir.symlink_metadata(name).map_err(io_error)?;
    let (id, metadata) = if metadata.is_dir() && !metadata.is_symlink() {
        let child = dir.open_dir_nofollow(name).map_err(io_error)?;
        (
            identity::directory(&child)?,
            child.dir_metadata().map_err(io_error)?,
        )
    } else {
        let file = open_regular(dir, name)?;
        (identity::file(&file)?, file.metadata().map_err(io_error)?)
    };
    Ok(Object {
        id,
        directory: metadata.is_dir(),
        size: metadata.len(),
        modified: metadata.modified().ok(),
        permissions: metadata.permissions(),
    })
}

fn validate_parent(root: &Path, parts: &[&str], expected: Identity) -> Result<(), Fault> {
    let current = path::descend(path::root(root)?, parts)?;
    if identity::directory(&current)? == expected {
        Ok(())
    } else {
        Err(changed())
    }
}

fn absent(dir: &Dir, name: &str) -> Result<(), Fault> {
    match dir.symlink_metadata(name) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
        Ok(_) => Err(Fault::new(
            ErrorCode::Conflict,
            "an entry already exists at this path",
        )),
    }
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
fn changed() -> Fault {
    Fault::new(ErrorCode::RevisionConflict, "copy source or parent changed")
}
fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "copy could not be confirmed; inspect the destination before retrying",
    )
}

fn move_unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "cross-filesystem move could not be confirmed; inspect both locations and Trash before retrying",
    )
}
