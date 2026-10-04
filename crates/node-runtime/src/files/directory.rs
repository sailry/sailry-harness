//! Bounded, ordered directory pages. No iterator or second read database is retained.
//! Listing behavior reviewed against harbor-file-service 727ce0a (Apache-2.0).
use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::Path,
};

use cap_std::fs::Dir;
use sailry_protocol::*;

use super::{Control, io_error, path};

pub(super) fn list(
    root: &Path,
    path: String,
    after: Option<&DirectoryCursor>,
    control: &Control,
) -> Result<Directory, Fault> {
    let parts = path::components(&path, true)?;
    if let Some(after) = after
        && (path::components(&after.name, false)?.len() != 1
            || after.revision.len() != 16
            || !after.revision.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid directory cursor",
        ));
    }
    let dir = path::descend(path::root(root)?, &parts)?;
    let revision = revision(&dir, root, &path)?;
    if after.is_some_and(|after| after.revision != revision) {
        return Err(changed());
    }
    let mut entries = BTreeMap::new();
    let mut unsupported_names = 0;
    for entry in dir.entries().map_err(io_error)? {
        control.check()?;
        let entry = entry.map_err(io_error)?;
        let Ok(name) = entry.file_name().into_string() else {
            unsupported_names += 1;
            continue;
        };
        if path::components(&name, false).is_err() {
            unsupported_names += 1;
            continue;
        }
        let metadata = dir.symlink_metadata(&name).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                changed()
            } else {
                io_error(error)
            }
        })?;
        let kind = if metadata.is_symlink() {
            EntryKind::Symlink
        } else if metadata.is_dir() {
            EntryKind::Directory
        } else if metadata.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let entry = FileEntry {
            name,
            kind,
            size: metadata.len(),
        };
        if after.is_some_and(|after| entry.sort_key() <= after.sort_key()) {
            continue;
        }
        let key = (kind != EntryKind::Directory, entry.name.clone());
        entries.insert(key, entry);
        // Keep only the smallest page and one lookahead, regardless of OS order.
        if entries.len() > MAX_DIRECTORY_ENTRIES + 1 {
            entries.pop_last();
        }
    }
    control.check()?;
    if revision != self::revision(&dir, root, &path)? {
        return Err(changed());
    }
    let truncated = entries.len() > MAX_DIRECTORY_ENTRIES;
    if truncated {
        entries.pop_last();
    }
    let next = truncated.then(|| {
        let entry = entries.last_key_value().unwrap().1;
        DirectoryCursor {
            revision: revision.clone(),
            directory: entry.kind == EntryKind::Directory,
            name: entry.name.clone(),
        }
    });
    Ok(Directory {
        path,
        entries: entries.into_values().collect(),
        truncated,
        unsupported_names,
        revision,
        next,
    })
}

fn revision(dir: &Dir, root: &Path, path: &str) -> Result<String, Fault> {
    let metadata = dir.dir_metadata().map_err(io_error)?;
    let mut revision = DefaultHasher::new();
    root.hash(&mut revision);
    path.hash(&mut revision);
    path::identity::directory(dir)?.hash(&mut revision);
    metadata.modified().map_err(io_error)?.hash(&mut revision);
    metadata.len().hash(&mut revision);
    // Opaque and read-only: restarting with a different implementation may
    // invalidate a cursor. Filesystem timestamps are not snapshot isolation.
    Ok(format!("{:016x}", revision.finish()))
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "directory changed; restart listing",
    )
}

#[cfg(test)]
mod tests;
