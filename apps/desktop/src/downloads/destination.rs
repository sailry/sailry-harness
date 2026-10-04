//! Desktop-local staging of a user-selected download destination.
use sailry_link::CancellationToken;
use std::{fs, io, path::PathBuf, time::SystemTime};
use tempfile::NamedTempFile;

pub(super) struct Destination {
    path: PathBuf,
    parent: PathBuf,
    expected: Option<Stamp>,
    staging: NamedTempFile,
}

#[derive(PartialEq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
}

impl Destination {
    pub fn prepare(path: PathBuf) -> Result<Self, &'static str> {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err("files_download_destination");
        }
        let parent = path.parent().unwrap().canonicalize().map_err(failed)?;
        let expected = stamp(&path)?;
        let staging = tempfile::Builder::new()
            .prefix(".sailry-download-")
            .tempfile_in(&parent)
            .map_err(failed)?;
        Ok(Self {
            path,
            parent,
            expected,
            staging,
        })
    }

    pub fn writer(&self) -> Result<fs::File, &'static str> {
        self.staging.as_file().try_clone().map_err(failed)
    }

    pub fn publish(self, cancel: &CancellationToken) -> Result<(), &'static str> {
        self.staging.as_file().sync_all().map_err(failed)?;
        if cancel.is_cancelled() {
            return Err("files_download_cancelled");
        }
        if self.path.parent().unwrap().canonicalize().map_err(failed)? != self.parent
            || stamp(&self.path)? != self.expected
        {
            return Err("files_download_conflict");
        }
        // Ordinary metadata conflict detection, not isolation from external writers.
        // Existing destinations were selected through the native save panel.
        let result = if self.expected.is_some() {
            self.staging.persist(&self.path)
        } else {
            self.staging.persist_noclobber(&self.path)
        };
        result.map(|_| ()).map_err(|error| {
            if error.error.kind() == io::ErrorKind::AlreadyExists {
                "files_download_conflict"
            } else {
                "files_download_destination"
            }
        })
    }
}

fn stamp(path: &std::path::Path) -> Result<Option<Stamp>, &'static str> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Err("files_download_destination"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failed(error)),
    };
    Ok(Some(Stamp {
        size: metadata.len(),
        modified: metadata.modified().map_err(failed)?,
        created: metadata.created().ok(),
    }))
}

fn failed(_: io::Error) -> &'static str {
    "files_download_destination"
}

#[cfg(test)]
mod tests;
