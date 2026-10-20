//! File identities and validation receipts shared by local model consumers.
use crate::{Error, File, Result, storage};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    path: PathBuf,
    files: Vec<Identity>,
    #[cfg(unix)]
    directory: (u64, u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    bytes: u64,
    modified: SystemTime,
    #[cfg(unix)]
    native: (u64, u64, i64, i64),
    #[cfg(not(unix))]
    created: Option<SystemTime>,
}

pub fn candidate(path: &Path, files: &[File<'_>]) -> Option<Stamp> {
    crate::validate(files).ok()?;
    let handle = storage::open(path, false).ok()?;
    let directory = fs::symlink_metadata(path).ok()?;
    if !directory.is_dir() || directory.is_symlink() || path.canonicalize().ok()? != path {
        return None;
    }
    let mut identities = Vec::with_capacity(files.len());
    for file in files {
        let metadata = handle.symlink_metadata(file.name).ok()?;
        if !metadata.is_file() || metadata.is_symlink() || metadata.len() != file.size {
            return None;
        }
        let metadata = fs::symlink_metadata(path.join(file.name)).ok()?;
        if !metadata.is_file() || metadata.is_symlink() || metadata.len() != file.size {
            return None;
        }
        #[cfg(unix)]
        let native = {
            use std::os::unix::fs::MetadataExt;
            (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            )
        };
        identities.push(Identity {
            bytes: metadata.len(),
            modified: metadata.modified().ok()?,
            #[cfg(unix)]
            native,
            #[cfg(not(unix))]
            created: metadata.created().ok(),
        });
    }
    #[cfg(unix)]
    let directory = {
        use std::os::unix::fs::MetadataExt;
        (directory.dev(), directory.ino())
    };
    Some(Stamp {
        path: path.to_owned(),
        files: identities,
        #[cfg(unix)]
        directory,
    })
}

pub fn stamp(path: &Path, files: &[File<'_>], marker: Option<&str>) -> Option<Stamp> {
    if marker.is_some_and(|expected| !marked(path, expected)) {
        return None;
    }
    candidate(path, files)
}

pub fn verified(path: &Path, files: &[File<'_>], marker: Option<&str>) -> Option<Stamp> {
    let before = stamp(path, files, marker)?;
    (crate::ready(path, files) && stamp(path, files, marker).as_ref() == Some(&before))
        .then_some(before)
}

fn marked(path: &Path, expected: &str) -> bool {
    let Ok(directory) = storage::open(path, false) else {
        return false;
    };
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let Ok(file) = directory.open_with("validated", &options) else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == expected.len() as u64)
    {
        return false;
    }
    let mut body = String::new();
    file.take(expected.len() as u64 + 1)
        .read_to_string(&mut body)
        .is_ok()
        && body == expected
}

pub fn write_marker(directory: &Dir, marker: &str) -> Result<()> {
    if marker.is_empty() || marker.len() > 1024 {
        return Err(Error::InvalidAsset);
    }
    storage::target(directory, "validated")?;
    let mut file = storage::temporary(directory)?;
    file.write_all(marker.as_bytes()).map_err(Error::storage)?;
    file.as_file().sync_all().map_err(Error::storage)?;
    file.replace("validated").map_err(Error::storage)?;
    Ok(())
}

pub fn mark(path: &Path, files: &[File<'_>], marker: &str, expected: &Stamp) -> Result<Stamp> {
    if !crate::ready(path, files) || candidate(path, files).as_ref() != Some(expected) {
        return Err(Error::InvalidAsset);
    }
    write_marker(&storage::open(path, false)?, marker)?;
    stamp(path, files, Some(marker))
        .filter(|actual| actual == expected)
        .ok_or(Error::InvalidAsset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn preserves_marker_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().canonicalize().unwrap();
        fs::write(path.join("model.onnx"), b"model").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"model"));
        let files = [File {
            name: "model.onnx",
            size: 5,
            sha256: &hash,
        }];
        let before = candidate(&path, &files).unwrap();
        for marker in ["a".repeat(64), "b".repeat(64)] {
            assert_eq!(mark(&path, &files, &marker, &before).unwrap(), before);
            assert_eq!(verified(&path, &files, Some(&marker)).unwrap(), before);
        }
        assert!(stamp(&path, &files, Some(&"a".repeat(64))).is_none());
    }

    #[test]
    fn rejects_changed_assets() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().canonicalize().unwrap();
        fs::write(path.join("model.onnx"), b"model").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"model"));
        let files = [File {
            name: "model.onnx",
            size: 5,
            sha256: &hash,
        }];
        let before = candidate(&path, &files).unwrap();
        fs::write(path.join("model.onnx"), b"other").unwrap();
        assert!(mark(&path, &files, "receipt", &before).is_err());
        fs::write(path.join("model.onnx"), b"shorter").unwrap();
        assert!(candidate(&path, &files).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_assets() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().canonicalize().unwrap();
        fs::write(path.join("source"), b"model").unwrap();
        std::os::unix::fs::symlink("source", path.join("model.onnx")).unwrap();
        let hash = format!("{:x}", Sha256::digest(b"model"));
        let files = [File {
            name: "model.onnx",
            size: 5,
            sha256: &hash,
        }];
        assert!(candidate(&path, &files).is_none());
        std::os::unix::fs::symlink("source", path.join("validated")).unwrap();
        assert!(write_marker(&storage::open(&path, false).unwrap(), "receipt").is_err());
    }
}
