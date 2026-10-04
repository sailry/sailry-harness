//! One native index lock shared by staging and commit publication.
use super::*;
use cap_std::fs::File;

pub(in crate::git) struct Lock {
    pub dir: Dir,
    pub file: File,
    pub published: bool,
}

impl Lock {
    pub fn acquire(repository: &Repository) -> Result<Self, Fault> {
        let location = location(repository)?;
        let dir = path::root(location.parent().unwrap())?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        cap_std::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let file = dir.open_with("index.lock", &options).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Fault::new(ErrorCode::Busy, "another Git index writer holds the lock")
            } else {
                io_error(error)
            }
        })?;
        Ok(Self {
            dir,
            file,
            published: false,
        })
    }

    pub fn owns(&self) -> bool {
        let same = || -> Result<bool, Fault> {
            let mut options = OpenOptions::new();
            options.read(true).follow(FollowSymlinks::No);
            #[cfg(unix)]
            cap_std::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
            let current = self
                .dir
                .open_with("index.lock", &options)
                .map_err(io_error)?;
            Ok(path::identity::file(&self.file)? == path::identity::file(&current)?)
        };
        same().unwrap_or(false)
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if !self.published && self.owns() {
            let _ = self.dir.remove_file("index.lock");
        }
    }
}
