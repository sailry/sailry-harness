//! Publish verified snapshots, retaining the previous package until replacement succeeds.
use super::Loaded;
use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub struct InstallError {
    pub recovery: Option<PathBuf>,
    message: String,
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl From<io::Error> for InstallError {
    fn from(error: io::Error) -> Self {
        Self {
            recovery: None,
            message: error.to_string(),
        }
    }
}

pub(super) fn install(loaded: &Loaded, root: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(root)?;
    let staging = tempfile::tempdir_in(root)?;
    for (relative, bytes) in &loaded.files {
        let path = staging.path().join(relative);
        fs::create_dir_all(path.parent().expect("verified theme resource path"))?;
        fs::write(path, bytes)?;
    }
    publish(staging.path(), &root.join(&loaded.manifest.id))
}

fn publish(staging: &Path, destination: &Path) -> Result<(), InstallError> {
    let metadata = match fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return fs::rename(staging, destination).map_err(Into::into);
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "installed theme directory changed",
        )
        .into());
    }
    let backup = tempfile::tempdir_in(destination.parent().expect("theme catalog root"))?;
    let previous = backup.path().join("package");
    fs::rename(destination, &previous)?;
    if let Err(error) = fs::rename(staging, destination) {
        if let Err(restore) = fs::rename(&previous, destination) {
            let recovery = backup.keep().join("package");
            return Err(InstallError {
                message: format!(
                    "could not publish theme: {error}; could not restore theme: {restore}; previous package retained at {}",
                    recovery.display()
                ),
                recovery: Some(recovery),
            });
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
