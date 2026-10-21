//! File identities and validation receipts shared by local model consumers.
use crate::{Error, File, Result, storage};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Binds a validation marker to the file identities whose contents were hashed.
const RECEIPT: &str = ".verified";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    path: PathBuf,
    files: Vec<Identity>,
    #[cfg(unix)]
    directory: (u64, u64),
}

impl Stamp {
    fn digest(&self, marker: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(marker.as_bytes());
        hash.update([0]);
        hash.update(self.path.as_os_str().as_encoded_bytes());
        hash.update([0]);
        #[cfg(unix)]
        {
            hash.update(self.directory.0.to_le_bytes());
            hash.update(self.directory.1.to_le_bytes());
        }
        for file in &self.files {
            hash.update(file.bytes.to_le_bytes());
            hash.update(instant(file.modified));
            #[cfg(unix)]
            {
                let (device, inode, seconds, nanoseconds) = file.native;
                hash.update(device.to_le_bytes());
                hash.update(inode.to_le_bytes());
                hash.update(seconds.to_le_bytes());
                hash.update(nanoseconds.to_le_bytes());
            }
            #[cfg(not(unix))]
            hash.update(file.created.map_or([0; 16], instant));
        }
        format!("{:x}", hash.finalize())
    }
}

fn instant(time: SystemTime) -> [u8; 16] {
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    }
    .to_le_bytes()
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

/// Hashes every file and records a receipt so unchanged files are not hashed again.
pub fn verified(path: &Path, files: &[File<'_>], marker: Option<&str>) -> Option<Stamp> {
    let before = stamp(path, files, marker)?;
    if !crate::ready(path, files) || stamp(path, files, marker).as_ref() != Some(&before) {
        return None;
    }
    if let Some(marker) = marker {
        // A read-only profile keeps its verified files and hashes them again next start.
        let _ = record(path, marker, &before);
    }
    Some(before)
}

/// Reports whether a receipt proves these marked files were already hashed.
pub fn recorded(path: &Path, marker: &str, stamp: &Stamp) -> bool {
    let digest = stamp.digest(marker);
    read(path, RECEIPT, digest.len()).as_deref() == Some(digest.as_str())
}

fn record(path: &Path, marker: &str, stamp: &Stamp) -> Result<()> {
    replace(
        &storage::open(path, false)?,
        RECEIPT,
        stamp.digest(marker).as_bytes(),
    )
}

fn marked(path: &Path, expected: &str) -> bool {
    read(path, "validated", expected.len()).as_deref() == Some(expected)
}

fn read(path: &Path, name: &str, length: usize) -> Option<String> {
    let directory = storage::open(path, false).ok()?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = directory.open_with(name, &options).ok()?;
    if !file
        .metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == length as u64)
    {
        return None;
    }
    let mut body = String::new();
    file.take(length as u64 + 1)
        .read_to_string(&mut body)
        .ok()?;
    Some(body)
}

pub fn write_marker(directory: &Dir, marker: &str) -> Result<()> {
    if marker.is_empty() || marker.len() > 1024 {
        return Err(Error::InvalidAsset);
    }
    replace(directory, "validated", marker.as_bytes())
}

fn replace(directory: &Dir, name: &str, body: &[u8]) -> Result<()> {
    storage::target(directory, name)?;
    let mut file = storage::temporary(directory)?;
    file.write_all(body).map_err(Error::storage)?;
    file.as_file().sync_all().map_err(Error::storage)?;
    file.replace(name).map_err(Error::storage)?;
    Ok(())
}

pub fn mark(path: &Path, files: &[File<'_>], marker: &str, expected: &Stamp) -> Result<Stamp> {
    if !crate::ready(path, files) || candidate(path, files).as_ref() != Some(expected) {
        return Err(Error::InvalidAsset);
    }
    // The receipt precedes the marker, so observers never see marked files without it.
    record(path, marker, expected)?;
    write_marker(&storage::open(path, false)?, marker)?;
    stamp(path, files, Some(marker))
        .filter(|actual| actual == expected)
        .ok_or(Error::InvalidAsset)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn recognizes_recorded_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().canonicalize().unwrap();
        fs::write(path.join("model.onnx"), b"model").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"model"));
        let files = [File {
            name: "model.onnx",
            size: 5,
            sha256: &hash,
        }];
        let marker = "a".repeat(64);
        let before = candidate(&path, &files).unwrap();
        assert_eq!(mark(&path, &files, &marker, &before).unwrap(), before);
        assert!(recorded(&path, &marker, &before));
        assert!(!recorded(&path, &"b".repeat(64), &before));
        fs::remove_file(path.join(RECEIPT)).unwrap();
        assert!(!recorded(&path, &marker, &before));
        assert_eq!(verified(&path, &files, Some(&marker)), Some(before.clone()));
        assert!(recorded(&path, &marker, &before));
        fs::write(path.join("model.onnx"), b"other").unwrap();
        let after = candidate(&path, &files).unwrap();
        assert_ne!(after, before);
        assert!(!recorded(&path, &marker, &after));
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
