//! SSH downloads publish through the same confined save target as editor uploads.
use super::{io_error, open_regular, path, save::Target};
use sailry_protocol::{ErrorCode, Fault};
use std::{io::Seek, path::Path};

pub(crate) const LIMIT: u64 = 1024 * 1024 * 1024;

pub(crate) struct Incoming {
    target: Target,
    pub file: std::fs::File,
}

fn directory(
    root: &Path,
    relative: &str,
    profile: Option<&Path>,
) -> Result<cap_std::fs::Dir, Fault> {
    let parts = path::entry_components(relative)?;
    let dir = path::descend(path::root(root)?, &parts[..parts.len() - 1])?;
    if profile.is_some_and(|profile| root.join(relative).starts_with(profile)) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "Node storage cannot be transferred over SSH",
        ));
    }
    Ok(dir)
}

pub(crate) fn source(
    root: &Path,
    relative: &str,
    profile: Option<&Path>,
) -> Result<std::fs::File, Fault> {
    let dir = directory(root, relative, profile)?;
    let file = open_regular(&dir, relative.rsplit('/').next().unwrap())?;
    if file.metadata().map_err(io_error)?.len() > LIMIT {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "SSH file exceeds the transfer limit",
        ));
    }
    Ok(file.into_std())
}

impl Incoming {
    pub(crate) fn prepare(
        root: &Path,
        relative: &str,
        profile: Option<&Path>,
    ) -> Result<Self, Fault> {
        let dir = directory(root, relative, profile)?;
        let target = Target::prepare(root, relative, None, LIMIT, None)?;
        let file = cap_tempfile::TempFile::new_anonymous(&dir)
            .map_err(io_error)?
            .into_std();
        Ok(Self { target, file })
    }

    pub(crate) fn publish(mut self, size: u64, revision: &str) -> Result<(), Fault> {
        self.file.rewind().map_err(io_error)?;
        self.target
            .write(
                self.file,
                size,
                revision,
                #[cfg(test)]
                &|_| {},
            )
            .map(|_| ())
    }
}
