use super::{Model, catalog, unavailable};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder};
use sailry_protocol::{Fault, models::Definition};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Stamp {
    path: PathBuf,
    files: Vec<Identity>,
    #[cfg(unix)]
    directory: (u64, u64),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Identity {
    bytes: u64,
    modified: SystemTime,
    #[cfg(unix)]
    native: (u64, u64, i64, i64),
    #[cfg(not(unix))]
    created: Option<SystemTime>,
}

pub(super) fn open(profile: &Path) -> Result<(PathBuf, Dir), Fault> {
    let profile = profile.canonicalize().map_err(|_| unavailable())?;
    let mut directory =
        Dir::open_ambient_dir(&profile, cap_std::ambient_authority()).map_err(|_| unavailable())?;
    for name in ["models", "embedding"] {
        let builder = DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use cap_std::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        match directory.create_dir_with(name, &builder) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(unavailable()),
        }
        directory = directory
            .open_dir_nofollow(name)
            .map_err(|_| unavailable())?;
    }
    Ok((profile.join("models/embedding"), directory))
}

pub(super) fn directory(root: &Path, model: Model) -> PathBuf {
    root.join(model.key().as_ref()).join(folder(model))
}

pub(super) fn stamp(root: &Path, model: Model, definition: &Definition) -> Option<Stamp> {
    let path = directory(root, model);
    let key = fs::symlink_metadata(root.join(model.key().as_ref())).ok()?;
    if !key.is_dir() || key.is_symlink() {
        return None;
    }
    let directory = fs::symlink_metadata(&path).ok()?;
    if !directory.is_dir() || directory.is_symlink() || path.canonicalize().ok()? != path {
        return None;
    }
    let mut files = Vec::new();
    for asset in &definition.assets {
        let metadata = fs::symlink_metadata(path.join(&asset.name)).ok()?;
        if !metadata.is_file() || metadata.is_symlink() || metadata.len() != asset.bytes {
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
        files.push(Identity {
            bytes: metadata.len(),
            modified: metadata.modified().ok()?,
            #[cfg(unix)]
            native,
            #[cfg(not(unix))]
            created: metadata.created().ok(),
        });
    }
    if matches!(model, Model::Registered { .. }) && !marked(&path, definition) {
        return None;
    }
    #[cfg(unix)]
    let directory = {
        use std::os::unix::fs::MetadataExt;
        (directory.dev(), directory.ino())
    };
    Some(Stamp {
        path,
        files,
        #[cfg(unix)]
        directory,
    })
}

pub(super) fn verified(root: &Path, model: Model, definition: &Definition) -> Option<Stamp> {
    let before = stamp(root, model, definition)?;
    let files = definition
        .assets
        .iter()
        .map(|asset| sailry_model_assets::File {
            name: &asset.name,
            size: asset.bytes,
            sha256: &asset.sha256,
        })
        .collect::<Vec<_>>();
    if !sailry_model_assets::ready(&directory(root, model), &files) {
        return None;
    }
    (stamp(root, model, definition).as_ref() == Some(&before)).then_some(before)
}

pub(super) fn remove(root: &Dir, model: Model) -> Result<(), Fault> {
    let directory = match root.open_dir_nofollow(model.key().as_ref()) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(unavailable()),
    };
    let revision = folder(model);
    match directory.symlink_metadata(&revision) {
        Ok(metadata) if metadata.is_dir() && !metadata.is_symlink() => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        _ => return Err(unavailable()),
    }
    directory
        .remove_dir_all(&revision)
        .map_err(|_| unavailable())
}

fn folder(model: Model) -> String {
    catalog::builtin(model).map_or_else(|| "files".into(), |definition| definition.source.revision)
}

fn digest(definition: &Definition) -> Result<String, Fault> {
    let body = serde_json::to_vec(definition).map_err(|_| unavailable())?;
    Ok(blake3::hash(&body).to_hex().to_string())
}

fn marked(path: &Path, definition: &Definition) -> bool {
    let Ok(directory) = Dir::open_ambient_dir(path, cap_std::ambient_authority()) else {
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
    if file.take(65).read_to_string(&mut body).is_err() {
        return false;
    }
    digest(definition).is_ok_and(|expected| expected == body)
}

pub(super) fn mark(root: &Dir, model: Model, definition: &Definition) -> Result<(), Fault> {
    if !matches!(model, Model::Registered { .. }) {
        return Ok(());
    }
    let directory = root
        .open_dir_nofollow(model.key().as_ref())
        .map_err(|_| unavailable())?
        .open_dir_nofollow(folder(model))
        .map_err(|_| unavailable())?;
    match directory.symlink_metadata("validated") {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        _ => return Err(unavailable()),
    }
    let mut file = cap_tempfile::TempFile::new(&directory).map_err(|_| unavailable())?;
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        file.as_file()
            .set_permissions(cap_std::fs::Permissions::from_mode(0o600))
            .map_err(|_| unavailable())?;
    }
    file.write_all(digest(definition)?.as_bytes())
        .map_err(|_| unavailable())?;
    file.as_file().sync_all().map_err(|_| unavailable())?;
    file.replace("validated").map_err(|_| unavailable())?;
    Ok(())
}
