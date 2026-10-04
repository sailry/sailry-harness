use super::*;
use sailry_link::CancellationToken;

/// One retained publication target shared by editor saves and staged uploads.
pub(in crate::files) struct Target {
    root: PathBuf,
    relative: String,
    dir: Dir,
    parent: identity::Identity,
    original: Option<Original>,
    limit: u64,
}

struct Original {
    revision: String,
    identity: identity::Identity,
    permissions: cap_std::fs::Permissions,
}

impl Target {
    pub(in crate::files) fn prepare(
        root: &Path,
        relative: &str,
        expected: Option<&str>,
        limit: u64,
        cancel: Option<&CancellationToken>,
    ) -> Result<Self, Fault> {
        let parts = path::components(relative, false)?;
        let (name, parents) = parts.split_last().unwrap();
        let dir = path::descend(path::root(root)?, parents)?;
        let parent = identity::directory(&dir)?;
        let original = if let Some(expected) = expected {
            let (file, revision) = current(&dir, name, limit, cancel)?;
            if revision != expected {
                return Err(conflict());
            }
            let permissions = file.metadata().map_err(io_error)?.permissions();
            if permissions.readonly() {
                return Err(Fault::new(ErrorCode::PermissionDenied, "file is read-only"));
            }
            Some(Original {
                revision,
                identity: identity::file(&file)?,
                permissions,
            })
        } else {
            match dir.symlink_metadata(name) {
                Ok(_) => return Err(conflict()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(error)),
            }
            None
        };
        Ok(Self {
            root: root.into(),
            relative: relative.into(),
            dir,
            parent,
            original,
            limit,
        })
    }

    pub(in crate::files) fn write(
        self,
        mut source: impl Read,
        size: u64,
        revision: &str,
        #[cfg(test)] hook: &dyn Fn(Phase),
    ) -> Result<FileWritten, Fault> {
        let name = self.relative.rsplit('/').next().unwrap();
        let id = if let Some(original) = &self.original {
            let mut temp = TempFile::new(&self.dir).map_err(io_error)?;
            temp.as_file()
                .set_permissions(original.permissions.clone())
                .map_err(io_error)?;
            content::copy(&mut source, &mut temp, size, revision)?;
            temp.as_file().sync_all().map_err(io_error)?;
            let id = identity::file(temp.as_file())?;
            #[cfg(test)]
            hook(Phase::Prepared);
            self.validate_parent()?;
            let (current, revision) = current(&self.dir, name, self.limit, None)?;
            if revision != original.revision
                || identity::file(&current)? != original.identity
                || current.metadata().map_err(io_error)?.permissions() != original.permissions
            {
                return Err(conflict());
            }
            drop(current);
            // The maintained library owns replacement and temporary-file cleanup.
            temp.replace(name).map_err(io_error)?;
            id
        } else {
            #[cfg(test)]
            hook(Phase::Prepared);
            self.validate_parent()?;
            let mut options = OpenOptions::new();
            options
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No);
            let mut file = self.dir.open_with(name, &options).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    conflict()
                } else {
                    io_error(error)
                }
            })?;
            let id = identity::file(&file).map_err(|_| unknown())?;
            // As for editor saves, failed exclusive creation may leave partial new
            // content. Report uncertainty; never delete a concurrently changed file.
            content::copy(&mut source, &mut file, size, revision).map_err(|_| unknown())?;
            file.sync_all().map_err(|_| unknown())?;
            id
        };
        #[cfg(test)]
        hook(Phase::Published);
        #[cfg(unix)]
        self.dir
            .try_clone()
            .and_then(|dir| dir.into_std_file().sync_all())
            .map_err(|_| unknown())?;
        self.validate_parent().map_err(|_| unknown())?;
        let (file, observed) =
            current(&self.dir, name, self.limit.max(size), None).map_err(|_| unknown())?;
        if identity::file(&file).map_err(|_| unknown())? != id || observed != revision {
            return Err(unknown());
        }
        Ok(FileWritten {
            path: self.relative,
            revision: revision.into(),
            size,
        })
    }

    fn validate_parent(&self) -> Result<(), Fault> {
        let parts = path::components(&self.relative, false)?;
        let current = path::descend(path::root(&self.root)?, &parts[..parts.len() - 1])?;
        if identity::directory(&current)? == self.parent {
            Ok(())
        } else {
            Err(conflict())
        }
    }
}

#[cfg(test)]
#[path = "target_tests.rs"]
mod tests;
