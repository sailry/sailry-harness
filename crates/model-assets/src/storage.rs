use crate::{Error, File, Result};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder, OpenOptions};
use cap_tempfile::TempFile;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Component, Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

pub(super) fn open(path: &Path, create: bool) -> Result<Dir> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| Error::Storage)?
            .join(path)
    };
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let absolute = {
        let mut absolute = absolute;
        // Apple application-support and temporary paths use these system aliases.
        for (alias, destination) in [("/var", "/private/var"), ("/tmp", "/private/tmp")] {
            let alias = Path::new(alias);
            if let Ok(relative) = absolute.strip_prefix(alias)
                && alias.is_symlink()
            {
                if alias.canonicalize().map_err(|_| Error::Storage)? != Path::new(destination) {
                    return Err(Error::Storage);
                }
                absolute = Path::new(destination).join(relative);
                break;
            }
        }
        absolute
    };
    let mut base = PathBuf::new();
    let mut names = Vec::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => base.push(component),
            Component::Normal(name) => names.push(name),
            _ => return Err(Error::Storage),
        }
    }
    if names.is_empty() {
        return Err(Error::Storage);
    }
    let mut directory =
        Dir::open_ambient_dir(base, cap_std::ambient_authority()).map_err(|_| Error::Storage)?;
    for name in names {
        match directory.open_dir_nofollow(name) {
            Ok(next) => {
                directory = next;
                continue;
            }
            Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Storage),
        }
        {
            let builder = DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use cap_std::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            match directory.create_dir_with(name, &builder) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(Error::Storage),
            }
        }
        directory = directory
            .open_dir_nofollow(name)
            .map_err(|_| Error::Storage)?;
    }
    Ok(directory)
}

pub(super) fn target(directory: &Dir, name: &str) -> Result<()> {
    match directory.symlink_metadata(name) {
        Ok(metadata) if metadata.is_file() && !metadata.is_symlink() => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(Error::Storage),
    }
}

pub(super) fn temporary(directory: &Dir) -> Result<TempFile<'_>> {
    let file = TempFile::new(directory).map_err(|_| Error::Storage)?;
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        file.as_file()
            .set_permissions(cap_std::fs::Permissions::from_mode(0o600))
            .map_err(|_| Error::Storage)?;
    }
    Ok(file)
}

pub(super) fn valid(
    directory: &Dir,
    specification: &File<'_>,
    cancel: Option<&CancellationToken>,
) -> bool {
    if target(directory, specification.name).is_err() {
        return false;
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let Ok(mut file) = directory.open_with(specification.name, &options) else {
        return false;
    };
    let Ok(before) = file.metadata() else {
        return false;
    };
    if !before.is_file() || before.len() != specification.size {
        return false;
    }
    let mut buffer = [0; 64 * 1024];
    let mut hash = Sha256::new();
    let mut received = 0u64;
    loop {
        if cancel.is_some_and(CancellationToken::is_cancelled) {
            return false;
        }
        let Ok(count) = file.read(&mut buffer) else {
            return false;
        };
        if count == 0 {
            break;
        }
        received = match received.checked_add(count as u64) {
            Some(size) if size <= specification.size => size,
            _ => return false,
        };
        hash.update(&buffer[..count]);
    }
    let Ok(after) = file.metadata() else {
        return false;
    };
    received == specification.size
        && before.len() == after.len()
        && before.modified().ok() == after.modified().ok()
        && format!("{:x}", hash.finalize()).eq_ignore_ascii_case(specification.sha256)
}
