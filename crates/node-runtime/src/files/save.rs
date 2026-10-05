//! Shared editor saves using cap-std and cap-tempfile on every platform.
//! Path and revision checks adapted from sailry-code 67ae9fa0:
//! sailry-code-secure-file/src/secure/write.rs (Apache-2.0).
//! Recheck before replacement; this is not isolation from external writers.
use super::{io_error, path};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use cap_tempfile::TempFile;
use path::identity;

use sailry_protocol::{ErrorCode, Fault, FileWritten, MAX_FILE_BYTES};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[path = "save/content.rs"]
pub(super) mod content;
#[path = "save/target.rs"]
mod target;
pub(super) use target::Target;

#[cfg(test)]
mod tests;

pub(crate) fn save(
    root: &Path,
    path: &str,
    text: &str,
    expected: Option<&str>,
) -> Result<FileWritten, Fault> {
    if text.len() > MAX_FILE_BYTES
        || text.contains('\0')
        || expected.is_some_and(|value| {
            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid file content or revision",
        ));
    }
    publish(
        root,
        path,
        text,
        expected,
        #[cfg(test)]
        &|_| {},
    )
}

/// Binary artifacts share the same create-only and revision-checked publication.
pub(crate) fn write_binary(
    root: &Path,
    path: &str,
    bytes: &[u8],
    expected: Option<&str>,
    limit: usize,
) -> Result<FileWritten, Fault> {
    if bytes.len() > limit
        || expected.is_some_and(|value| {
            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid artifact size or revision",
        ));
    }
    if expected.is_none() {
        path::create_parents(root, path)?;
    }
    let revision = blake3::hash(bytes).to_hex().to_string();
    Target::prepare(root, path, expected, limit as u64, None)?.write(
        bytes,
        bytes.len() as u64,
        &revision,
        #[cfg(test)]
        &|_| {},
    )
}

#[cfg(test)]
pub(super) enum Phase {
    Prepared,
    Published,
}

fn publish(
    root: &Path,
    relative: &str,
    text: &str,
    expected: Option<&str>,
    #[cfg(test)] hook: &dyn Fn(Phase),
) -> Result<FileWritten, Fault> {
    if expected.is_none() {
        path::create_parents(root, relative)?;
    }
    let revision = blake3::hash(text.as_bytes()).to_hex().to_string();
    Target::prepare(root, relative, expected, MAX_FILE_BYTES as u64, None)?.write(
        text.as_bytes(),
        text.len() as u64,
        &revision,
        #[cfg(test)]
        hook,
    )
}

fn current(
    dir: &Dir,
    name: &str,
    limit: u64,
    cancel: Option<&sailry_link::CancellationToken>,
) -> Result<(File, String), Fault> {
    let metadata = dir.symlink_metadata(name).map_err(io_error)?;
    if !metadata.is_file() || metadata.is_symlink() {
        return Err(conflict());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    cap_std::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let mut file = dir.open_with(name, &options).map_err(io_error)?;
    identity::file(&file)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(conflict());
    }
    let revision = content::revision(&mut file, limit, cancel)?;
    Ok((file, revision))
}

fn conflict() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "file changed outside this operation",
    )
}

fn unknown() -> Fault {
    Fault::new(
        ErrorCode::OutcomeUnknown,
        "file save could not be confirmed; inspect the file before retrying",
    )
}
