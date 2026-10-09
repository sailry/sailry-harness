use sailry_protocol::{Fault, models::Definition};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    root: PathBuf,
    files: Vec<Identity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    bytes: u64,
    modified: SystemTime,
    #[cfg(unix)]
    native: (u64, u64, i64, i64),
}

pub(super) fn stamp(path: &Path, definition: &Definition) -> Option<Stamp> {
    if !marked(path, definition) {
        return None;
    }
    candidate(path, definition)
}

pub(super) fn candidate(path: &Path, definition: &Definition) -> Option<Stamp> {
    let root = fs::symlink_metadata(path).ok()?;
    if !root.is_dir() || root.is_symlink() || path.canonicalize().ok()? != path {
        return None;
    }
    let mut files = Vec::new();
    for asset in &definition.assets {
        let file = fs::symlink_metadata(path.join(&asset.name)).ok()?;
        if !file.is_file() || file.is_symlink() || file.len() != asset.bytes {
            return None;
        }
        #[cfg(unix)]
        let native = {
            use std::os::unix::fs::MetadataExt;
            (file.dev(), file.ino(), file.ctime(), file.ctime_nsec())
        };
        files.push(Identity {
            bytes: file.len(),
            modified: file.modified().ok()?,
            #[cfg(unix)]
            native,
        });
    }
    Some(Stamp {
        root: path.to_owned(),
        files,
    })
}

pub(super) fn verified(path: &Path, definition: &Definition) -> Option<Stamp> {
    let before = stamp(path, definition)?;
    (ready(path, definition) && stamp(path, definition).as_ref() == Some(&before)).then_some(before)
}

fn ready(path: &Path, definition: &Definition) -> bool {
    let files = definition
        .assets
        .iter()
        .map(|asset| sailry_model_assets::File {
            name: &asset.name,
            size: asset.bytes,
            sha256: &asset.sha256,
        })
        .collect::<Vec<_>>();
    sailry_model_assets::ready(path, &files)
}

fn marked(path: &Path, definition: &Definition) -> bool {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
    let Ok(directory) = crate::files::path::root(path) else {
        return false;
    };
    let mut options = cap_std::fs::OpenOptions::new();
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
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() == 64)
    {
        return false;
    }
    let mut body = String::new();
    file.take(65).read_to_string(&mut body).is_ok() && body == super::catalog::id(definition)
}

pub(super) fn mark(path: &Path, definition: &Definition, expected: &Stamp) -> Result<Stamp, Fault> {
    if !ready(path, definition) || candidate(path, definition).as_ref() != Some(expected) {
        return Err(Fault::invalid("OCR assets changed during validation"));
    }
    let directory = crate::files::path::root(path)?;
    match directory.symlink_metadata("validated") {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        _ => {
            return Err(Fault::invalid(
                "OCR validation marker must be a regular file",
            ));
        }
    }
    let mut file = cap_tempfile::TempFile::new(&directory).map_err(super::failure)?;
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        file.as_file()
            .set_permissions(cap_std::fs::Permissions::from_mode(0o600))
            .map_err(super::failure)?;
    }
    file.write_all(super::catalog::id(definition).as_bytes())
        .map_err(super::failure)?;
    file.as_file().sync_all().map_err(super::failure)?;
    file.replace("validated").map_err(super::failure)?;
    stamp(path, definition)
        .filter(|actual| actual == expected)
        .ok_or_else(|| Fault::invalid("OCR assets changed during validation"))
}

pub(super) fn open(profile: &Path) -> Result<PathBuf, Fault> {
    let profile = profile.canonicalize().map_err(super::failure)?;
    for path in [profile.join("models"), profile.join("models/ocr")] {
        crate::profile::private_directory(&path).map_err(super::failure)?;
        let metadata = fs::symlink_metadata(&path).map_err(super::failure)?;
        if metadata.is_symlink() || !metadata.is_dir() {
            return Err(Fault::invalid("OCR storage must be a directory"));
        }
    }
    Ok(profile.join("models/ocr"))
}
