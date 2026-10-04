//! Source revisions bind retained filesystem identity and ordinary metadata.
//! They are not authorization tokens or isolation from external writers.
use cap_std::fs::{Dir, File};
use sailry_protocol::{ErrorCode, Fault};
use std::path::Path;

use super::{io_error, open_regular, path, save::content};

pub(super) fn current(root: &Path, relative: &str) -> Result<String, Fault> {
    let parts = path::components(relative, false)?;
    let (name, parents) = parts.split_last().unwrap();
    let root = path::root(root)?;
    let dir = path::descend(root.try_clone().map_err(io_error)?, parents)?;
    retained(&root, &dir, &open_regular(&dir, name)?, relative)
}

pub(super) fn retained(
    root: &Dir,
    parent: &Dir,
    file: &File,
    relative: &str,
) -> Result<String, Fault> {
    let metadata = file.metadata().map_err(io_error)?;
    #[cfg(unix)]
    let platform = {
        use cap_std::fs::MetadataExt;
        (metadata.mode(), metadata.ctime(), metadata.ctime_nsec())
    };
    #[cfg(not(unix))]
    let platform = metadata.permissions().readonly();
    let mut hasher = blake3::Hasher::new();
    serde_json::to_writer(
        &mut hasher,
        &(
            "file-source-v1",
            relative,
            path::identity::directory(root)?,
            path::identity::directory(parent)?,
            path::identity::file(file)?,
            metadata.len(),
            time(metadata.modified().ok()),
            time(metadata.created().ok()),
            platform,
        ),
    )
    .map_err(|_| {
        Fault::new(
            ErrorCode::Internal,
            "file source revision could not be encoded",
        )
    })?;
    Ok(hasher.finalize().to_hex().to_string())
}

pub(super) fn verify(
    root_path: &Path,
    relative: &str,
    revision: &str,
    stamp: &str,
) -> Result<(), Fault> {
    let parts = path::entry_components(relative)?;
    let (name, parents) = parts.split_last().unwrap();
    let root = path::root(root_path)?;
    let parent = path::descend(root.try_clone().map_err(io_error)?, parents)?;
    let mut file = open_regular(&parent, name)?;
    if retained(&root, &parent, &file, relative)? != stamp {
        return Err(changed());
    }
    let size = file.metadata().map_err(io_error)?.len();
    if content::revision(&mut file, size, None)? != revision
        || retained(&root, &parent, &file, relative)? != stamp
        || current(root_path, relative)? != stamp
    {
        return Err(changed());
    }
    Ok(())
}

fn time(value: Option<cap_std::time::SystemTime>) -> Option<(bool, u64, u32)> {
    value.map(|time| {
        let (negative, duration) = match time.into_std().duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => (false, duration),
            Err(error) => (true, error.duration()),
        };
        (negative, duration.as_secs(), duration.subsec_nanos())
    })
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "file source changed since download",
    )
}
