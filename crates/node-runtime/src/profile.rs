use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use crate::Error;

#[cfg(windows)]
pub(crate) mod windows;

/// Shared default for Desktop and Host. Resolving it does not create or open a profile.
pub fn default_data_dir() -> Result<PathBuf, Error> {
    from_home(dirs::home_dir())
}

fn from_home(directory: Option<PathBuf>) -> Result<PathBuf, Error> {
    directory
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(".sailry"))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "user home directory is unavailable; specify --data-dir",
            )
            .into()
        })
}

/// The file must remain linked: deleting it would let another owner lock a new inode.
pub(crate) struct Profile {
    pub(crate) path: PathBuf,
    _lock: File,
}

impl Profile {
    pub(crate) fn acquire(path: &Path) -> Result<Self, Error> {
        if !path.is_absolute() {
            return Err(invalid(path, "an absolute data directory is required"));
        }
        private_directory(path)?;
        let path = path.canonicalize()?;
        let lock_path = path.join("node.lock");
        #[cfg(windows)]
        {
            windows::validate_existing_file(&lock_path)?;
            windows::validate_existing_file(&path.join("link.key"))?;
        }
        if let Ok(metadata) = lock_path.symlink_metadata()
            && (!metadata.is_file() || metadata.is_symlink())
        {
            return Err(invalid(&lock_path, "lock must be a regular file"));
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options.open(&lock_path)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(Error::ProfileInUse(path)),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        let profile = Self { path, _lock: lock };
        if profile.path.join("peers.json").exists() {
            return Err(invalid(
                &profile.path,
                "a controller profile cannot be opened as a Node",
            ));
        }
        // Prepare only storage owned by this stage; Link and business stores follow in P2.
        private_directory(&profile.path.join("storage"))?;
        Ok(profile)
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        // A concurrent spawn can briefly inherit a duplicate before exec closes it.
        // Ownership ends here even while that duplicate handle remains open.
        let _ = self._lock.unlock();
    }
}

fn invalid(path: &Path, reason: &'static str) -> Error {
    Error::InvalidProfile {
        path: path.to_owned(),
        reason,
    }
}

fn private_directory(path: &Path) -> Result<(), Error> {
    #[cfg(windows)]
    return windows::directory(path);

    #[cfg(not(windows))]
    {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
        let metadata = path.symlink_metadata()?;
        if !metadata.is_dir() || metadata.is_symlink() {
            return Err(invalid(path, "data directory must not be a symbolic link"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            // SAFETY: geteuid has no arguments or memory safety preconditions.
            if metadata.uid() != unsafe { libc::geteuid() } {
                return Err(invalid(
                    path,
                    "data directory must belong to the current user",
                ));
            }
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(invalid(
                    path,
                    "data directory permissions must exclude group and other users",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
