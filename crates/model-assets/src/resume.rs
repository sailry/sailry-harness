//! Private checkpoints are derived from immutable asset identities.
use crate::{Download, Error, Result, storage};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use reqwest::{Response, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;

pub(super) async fn lock(
    path: &Path,
    name: &str,
    cancel: &CancellationToken,
) -> Result<OwnedMutexGuard<()>> {
    type Locks = BTreeMap<PathBuf, Weak<AsyncMutex<()>>>;
    static LOCKS: OnceLock<Mutex<Locks>> = OnceLock::new();
    let key = path.canonicalize().map_err(Error::storage)?.join(name);
    let lock = {
        let mut locks = LOCKS
            .get_or_init(Mutex::default)
            .lock()
            .map_err(Error::storage)?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        let lock = locks
            .get(&key)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| Arc::new(AsyncMutex::new(())));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Error::Cancelled),
        lock = lock.lock_owned() => Ok(lock),
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Validator {
    Etag(String),
    Modified(String),
}

impl Validator {
    fn value(&self) -> &str {
        match self {
            Self::Etag(value) | Self::Modified(value) => value,
        }
    }
    fn valid(&self) -> bool {
        let value = self.value();
        !value.is_empty()
            && value.len() <= 1024
            && header::HeaderValue::from_str(value).is_ok()
            && match self {
                Self::Etag(_) => value.starts_with('"') && value.ends_with('"'),
                Self::Modified(_) => true,
            }
    }
    fn matches(&self, response: &Response) -> bool {
        let name = match self {
            Self::Etag(_) => header::ETAG,
            Self::Modified(_) => header::LAST_MODIFIED,
        };
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            == Some(self.value())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    version: u8,
    endpoint: String,
    received: u64,
    sha256: String,
    validator: Option<Validator>,
}

pub(super) struct Part {
    pub file: std::fs::File,
    pub received: u64,
    pub hash: Sha256,
    directory: Dir,
    name: String,
    marker: String,
    endpoint: String,
    validator: Option<Validator>,
}

impl Part {
    pub fn open(
        directory: Dir,
        download: &Download<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        let specification = download.file;
        let name = format!(
            ".{}-{}-{}.part",
            specification.name,
            specification.size,
            specification.sha256.to_ascii_lowercase()
        );
        let marker = format!("{name}.json");
        storage::target(&directory, &name)?;
        storage::target(&directory, &marker)?;
        let endpoint = format!("{:x}", Sha256::digest(download.url.as_str().as_bytes()));
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NONBLOCK);
        }
        let mut file = directory
            .open_with(&name, &options)
            .map_err(Error::storage)?
            .into_std();
        if !file.metadata().map_err(Error::storage)?.is_file() {
            return Err(Error::Storage(
                "model asset checkpoint is not a regular file".into(),
            ));
        }
        let mut read = OpenOptions::new();
        read.read(true).follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            read.custom_flags(libc::O_NONBLOCK);
        }
        let checkpoint = directory
            .open_with(&marker, &read)
            .ok()
            .and_then(|file| {
                if !file.metadata().ok()?.is_file() {
                    return None;
                }
                let mut bytes = Vec::new();
                file.take(4097).read_to_end(&mut bytes).ok()?;
                (bytes.len() <= 4096)
                    .then(|| serde_json::from_slice::<Checkpoint>(&bytes).ok())
                    .flatten()
            })
            .filter(|checkpoint| {
                checkpoint.version == 1
                    && checkpoint.endpoint == endpoint
                    && checkpoint.received <= specification.size
                    && checkpoint.received
                        <= file.metadata().map(|metadata| metadata.len()).unwrap_or(0)
                    && checkpoint.validator.as_ref().is_some_and(Validator::valid)
            });
        let mut received = checkpoint
            .as_ref()
            .map_or(0, |checkpoint| checkpoint.received);
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut remaining = received;
        while remaining > 0 {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let limit = remaining.min(buffer.len() as u64) as usize;
            let count = file.read(&mut buffer[..limit]).map_err(Error::storage)?;
            if count == 0 {
                return Err(Error::Storage(
                    "model asset checkpoint ended unexpectedly".into(),
                ));
            }
            hash.update(&buffer[..count]);
            remaining -= count as u64;
        }
        if checkpoint
            .as_ref()
            .is_some_and(|checkpoint| format!("{:x}", hash.clone().finalize()) != checkpoint.sha256)
        {
            received = 0;
            hash = Sha256::new();
        }
        file.set_len(received).map_err(Error::storage)?;
        file.seek(SeekFrom::Start(received))
            .map_err(Error::storage)?;
        Ok(Self {
            file,
            received,
            hash,
            directory,
            name,
            marker,
            endpoint,
            validator: (received > 0)
                .then(|| checkpoint.and_then(|checkpoint| checkpoint.validator))
                .flatten(),
        })
    }

    pub fn range(&self) -> Option<&str> {
        self.validator
            .as_ref()
            .filter(|_| self.received > 0)
            .map(Validator::value)
    }

    pub fn matches(&self, response: &Response) -> bool {
        self.validator
            .as_ref()
            .is_some_and(|validator| validator.matches(response))
    }

    pub fn reset(&mut self) -> Result<()> {
        self.file.set_len(0).map_err(Error::storage)?;
        self.file.seek(SeekFrom::Start(0)).map_err(Error::storage)?;
        self.received = 0;
        self.hash = Sha256::new();
        self.validator = None;
        Ok(())
    }

    pub fn headers(&mut self, response: &Response) {
        self.validator = response
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(|value| Validator::Etag(value.into()))
            .filter(Validator::valid)
            .or_else(|| {
                response
                    .headers()
                    .get(header::LAST_MODIFIED)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| Validator::Modified(value.into()))
                    .filter(Validator::valid)
            });
    }

    pub fn checkpoint(&self) -> Result<()> {
        let bytes = serde_json::to_vec(&Checkpoint {
            version: 1,
            endpoint: self.endpoint.clone(),
            received: self.received,
            sha256: format!("{:x}", self.hash.clone().finalize()),
            validator: self.validator.clone(),
        })
        .map_err(Error::storage)?;
        let temporary = storage::temporary(&self.directory)?;
        temporary
            .as_file()
            .write_all(&bytes)
            .map_err(Error::storage)?;
        temporary.as_file().sync_all().map_err(Error::storage)?;
        storage::target(&self.directory, &self.marker)?;
        temporary.replace(&self.marker).map_err(Error::storage)?;
        Ok(())
    }

    pub fn publish(self, target: &str) -> Result<()> {
        drop(self.file);
        storage::target(&self.directory, target)?;
        self.directory
            .rename(&self.name, &self.directory, target)
            .map_err(Error::storage)?;
        match self.directory.remove_file(&self.marker) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::storage(error)),
        }
        Ok(())
    }

    pub fn discard(self) -> Result<()> {
        drop(self.file);
        for name in [&self.name, &self.marker] {
            match self.directory.remove_file(name) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(Error::storage(error)),
            }
        }
        Ok(())
    }
}

pub(super) fn range(response: &Response, start: u64, size: u64) -> bool {
    if start >= size {
        return false;
    }
    let Some(value) = response
        .headers()
        .get(header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some((range, total)) = value
        .strip_prefix("bytes ")
        .and_then(|value| value.split_once('/'))
    else {
        return false;
    };
    let Some((first, last)) = range.split_once('-') else {
        return false;
    };
    first.parse::<u64>() == Ok(start)
        && last
            .parse::<u64>()
            .ok()
            .and_then(|last| last.checked_add(1))
            == Some(size)
        && total.parse::<u64>() == Ok(size)
        && response
            .content_length()
            .is_none_or(|length| length == size - start)
}
