//! Stage/unstage behavior references sailry-code 67ae9fa0
//! sailry-git/src/service.rs (Apache-2.0). Publication uses an owned index lock
//! and a separate candidate index, preserving unrelated external Git updates.
use super::{fault, io_error, path};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use git2::Repository;
use sailry_protocol::*;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

mod lock;
#[cfg(unix)]
mod native;
pub(super) use lock::Lock;
const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn revision(repository: &Repository) -> Result<String, Fault> {
    let location = location(repository)?;
    let dir = path::root(location.parent().unwrap())?;
    Ok(digest(read(&dir, "index", MAX_INDEX_BYTES)?.as_deref()))
}

pub(super) fn location(repository: &Repository) -> Result<PathBuf, Fault> {
    let index = repository.index().map_err(fault)?;
    let location = index
        .path()
        .ok_or_else(|| Fault::invalid("repository has no file-backed index"))?;
    if location.file_name().is_none_or(|name| name != "index") {
        return Err(Fault::invalid("unsupported repository index path"));
    }
    Ok(location.to_path_buf())
}

fn read(dir: &Dir, name: &str, limit: usize) -> Result<Option<Vec<u8>>, Fault> {
    match dir.symlink_metadata(name) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(error)),
        Ok(metadata) if !metadata.is_file() || metadata.is_symlink() => {
            return Err(Fault::invalid("expected a regular Git resource"));
        }
        _ => {}
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    cap_std::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = dir.open_with(name, &options).map_err(io_error)?;
    if !file.metadata().map_err(io_error)?.is_file() {
        return Err(Fault::invalid("Git resource type changed"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limit {
        return Err(Fault::invalid(
            "Git resource exceeds the operation size limit",
        ));
    }
    Ok(Some(bytes))
}

fn digest(bytes: Option<&[u8]>) -> String {
    bytes
        .map(|bytes| blake3::hash(bytes).to_hex().to_string())
        .unwrap_or_else(|| "absent".into())
}

pub(crate) fn update(
    root: &Path,
    paths: &[String],
    operation: GitIndexChange,
    expected: &str,
    head: Option<&str>,
    id: RequestId,
) -> Result<GitIndex, Fault> {
    if paths.is_empty()
        || paths.len() > MAX_GIT_ENTRIES
        || !(expected == "absent"
            || (expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit())))
    {
        return Err(Fault::invalid("invalid Git index update"));
    }
    for value in paths {
        super::valid_path(value)?;
    }
    #[cfg(unix)]
    {
        native::update(root, paths, operation, expected, head, id)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, operation, head, id);
        Err(Fault::invalid(
            "Git index publication is unsupported on this platform",
        ))
    }
}

pub(crate) fn edit(
    root: &Path,
    path: &str,
    expected: &str,
    head: Option<&str>,
    id: RequestId,
    expected_source: &str,
    transform: impl FnOnce(&str) -> Result<String, Fault>,
) -> Result<GitIndex, Fault> {
    super::valid_path(path)?;
    if !(expected == "absent"
        || (expected.len() == 64 && expected.bytes().all(|byte| byte.is_ascii_hexdigit())))
    {
        return Err(Fault::invalid("invalid Git index revision"));
    }
    #[cfg(unix)]
    {
        native::edit(root, path, expected, head, id, expected_source, transform)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, head, id, expected_source, transform);
        Err(Fault::invalid(
            "Git index publication is unsupported on this platform",
        ))
    }
}
