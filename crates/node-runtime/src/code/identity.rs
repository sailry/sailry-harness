use sailry_protocol::{ErrorCode, Fault};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, Metadata},
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

const CAPACITY: usize = 512;
const MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Cache {
    entries: Mutex<BTreeMap<PathBuf, Entry>>,
    #[cfg(test)]
    reads: std::sync::atomic::AtomicUsize,
}

struct Entry {
    stamp: Stamp,
    revision: String,
}

#[derive(PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
    #[cfg(not(unix))]
    created: Option<SystemTime>,
}

impl Stamp {
    fn read(metadata: Metadata) -> Result<Self, Fault> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        if !metadata.is_file() {
            return Err(Fault::invalid("language executable is not a regular file"));
        }
        if metadata.len() > MAX_BYTES {
            return Err(Fault::invalid(
                "language executable exceeds the identity limit",
            ));
        }
        Ok(Self {
            size: metadata.len(),
            modified: metadata.modified().map_err(unavailable)?,
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
            #[cfg(not(unix))]
            created: metadata.created().ok(),
        })
    }
}

impl Cache {
    pub fn revision(&self, executable: &Path) -> Result<String, Fault> {
        let path = executable.canonicalize().map_err(unavailable)?;
        let mut entries = self
            .entries
            .lock()
            .expect("language identity lock is healthy");
        let stamp = Stamp::read(path.metadata().map_err(unavailable)?)?;
        if let Some(entry) = entries.get(&path)
            && entry.stamp == stamp
        {
            return Ok(entry.revision.clone());
        }
        let mut file = File::open(&path).map_err(unavailable)?;
        if Stamp::read(file.metadata().map_err(unavailable)?)? != stamp {
            return Err(changed());
        }
        let revision = self.digest(&mut file)?;
        if Stamp::read(file.metadata().map_err(unavailable)?)? != stamp
            || Stamp::read(path.metadata().map_err(unavailable)?)? != stamp
        {
            return Err(changed());
        }
        if entries.len() >= CAPACITY && !entries.contains_key(&path) {
            entries.pop_first();
        }
        entries.insert(
            path,
            Entry {
                stamp,
                revision: revision.clone(),
            },
        );
        Ok(revision)
    }

    fn digest(&self, file: &mut File) -> Result<String, Fault> {
        #[cfg(test)]
        self.reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0;
        loop {
            let count = file.read(&mut buffer).map_err(unavailable)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > MAX_BYTES {
                return Err(Fault::invalid(
                    "language executable exceeds the identity limit",
                ));
            }
            digest.update(&buffer[..count]);
        }
        Ok(format!("{:x}", digest.finalize()))
    }

    #[cfg(test)]
    pub fn reads(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn unavailable(error: std::io::Error) -> Fault {
    Fault::invalid(format!("language executable is unavailable: {error}"))
}

fn changed() -> Fault {
    Fault::new(
        ErrorCode::RevisionConflict,
        "language executable changed while reading its identity",
    )
}

#[cfg(test)]
mod tests;
