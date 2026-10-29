//! Copy a closed native database family after the lifecycle owner drains its handles.
use crate::Error;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub(in crate::store) struct Preview {
    path: PathBuf,
    directory: tempfile::TempDir,
}

impl Preview {
    pub(in crate::store) fn path(&self) -> &Path {
        &self.path
    }
}

pub(in crate::store) fn preview(path: &Path) -> Result<Preview, Error> {
    let parent = path.parent().ok_or_else(unavailable)?;
    snapshot(path, parent)
}

fn snapshot(path: &Path, parent: &Path) -> Result<Preview, Error> {
    let mut temporary = tempfile::Builder::new();
    temporary.prefix(".upgrade-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = temporary.tempdir_in(parent)?;
    crate::profile::private_directory(directory.path())?;
    let target = directory
        .path()
        .join(path.file_name().ok_or_else(unavailable)?);
    copy(path, &target)?;
    Ok(Preview {
        path: target,
        directory,
    })
}

/// The caller guarantees that no native database, connection or statement is alive.
pub(in crate::store) fn copy(source: &Path, target: &Path) -> Result<(), Error> {
    let source_parent = source.parent().ok_or_else(unavailable)?;
    let target_parent = target.parent().ok_or_else(unavailable)?;
    let source_dir = Dir::open_ambient_dir(source_parent, cap_std::ambient_authority())?;
    let target_dir = Dir::open_ambient_dir(target_parent, cap_std::ambient_authority())?;
    for (index, (source_file, target_file)) in crate::profile::database_files(source)
        .into_iter()
        .zip(crate::profile::database_files(target))
        .enumerate()
    {
        let source_file = source_file.file_name().ok_or_else(unavailable)?;
        let target_file = target_file.file_name().ok_or_else(unavailable)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut input = match source_dir.open_with(source_file, &options) {
            Ok(file) => file.into_std(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && index >= 2 => {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let metadata = input.metadata()?;
        if !metadata.is_file() {
            return Err(unavailable());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err(unavailable());
            }
        }
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        cap_std::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut output = target_dir.open_with(target_file, &options)?.into_std();
        let count = std::io::copy(&mut input, &mut output)?;
        let after = input.metadata()?;
        if count != metadata.len()
            || after.len() != metadata.len()
            || after.modified().ok() != metadata.modified().ok()
        {
            return Err(Error::Worker(
                "database changed during snapshot; existing data was not changed".into(),
            ));
        }
        output.sync_all()?;
    }
    Ok(())
}

pub(in crate::store) fn save(path: &Path) -> Result<PathBuf, Error> {
    let directory = path.parent().ok_or_else(unavailable)?.join("backups");
    crate::profile::private_directory(&directory)?;
    let snapshot = snapshot(path, &directory)?;
    let target = directory.join(format!("node-before-upgrade-{}", uuid::Uuid::new_v4()));
    std::fs::rename(snapshot.directory.path(), &target)?;
    Ok(target.join(path.file_name().ok_or_else(unavailable)?))
}

/// Remove only a closed Node database family after its complete backup is saved.
pub(in crate::store) fn clear(path: &Path) -> Result<(), Error> {
    for file in crate::profile::database_files(path) {
        match std::fs::remove_file(file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn unavailable() -> Error {
    Error::Worker("database snapshot requires private regular files".into())
}
