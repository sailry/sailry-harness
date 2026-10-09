use super::*;
use cap_std::fs::{Metadata, MetadataExt};
use std::sync::{Arc, Mutex, OnceLock};

type Slot = Arc<Mutex<Option<Snapshot>>>;

#[derive(Default)]
pub(super) struct Cache {
    entries: Mutex<BTreeMap<PathBuf, Slot>>,
    #[cfg(test)]
    pub scans: std::sync::atomic::AtomicUsize,
}

#[derive(Default)]
pub(super) struct Snapshot {
    pub paths: BTreeSet<PathBuf>,
    pub witnesses: BTreeMap<PathBuf, Witness>,
    pub targets: BTreeMap<PathBuf, Option<PathBuf>>,
    pub stable: bool,
}

#[derive(PartialEq, Eq)]
pub(super) enum Witness {
    Present(Stamp),
    Missing,
    Unavailable(std::io::ErrorKind),
}

#[derive(PartialEq, Eq)]
pub(super) struct Stamp {
    size: u64,
    identity: (u64, u64, u32),
    modified: (i64, i64),
    changed: (i64, i64),
}

impl Witness {
    pub fn read(metadata: std::io::Result<Metadata>) -> Self {
        match metadata {
            Ok(metadata) => Self::Present(Stamp {
                size: metadata.len(),
                identity: (metadata.dev(), metadata.ino(), metadata.mode()),
                modified: (metadata.mtime(), metadata.mtime_nsec()),
                changed: (metadata.ctime(), metadata.ctime_nsec()),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unavailable(error.kind()),
        }
    }

    fn current(path: &Path) -> Option<Self> {
        match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => {
                let directory = crate::files::path::root(parent).ok()?;
                Some(Self::read(directory.symlink_metadata(name)))
            }
            _ => {
                let directory = crate::files::path::root(path).ok()?;
                Some(Self::read(directory.dir_metadata()))
            }
        }
    }
}

impl Snapshot {
    pub fn current(&self) -> bool {
        self.stable
            && self
                .witnesses
                .iter()
                .all(|(path, witness)| Witness::current(path).as_ref() == Some(witness))
            && self
                .targets
                .iter()
                .all(|(path, target)| path.canonicalize().ok().as_ref() == target.as_ref())
    }
}

impl Cache {
    pub fn global() -> &'static Self {
        static CACHE: OnceLock<Cache> = OnceLock::new();
        CACHE.get_or_init(Self::default)
    }

    pub fn discover(&self, root: &Path) -> Result<BTreeSet<PathBuf>, Fault> {
        let slot = {
            let mut entries = self.entries.lock().expect("Git discovery lock is healthy");
            if !entries.contains_key(root) && entries.len() >= 16 {
                entries.pop_first();
            }
            entries.entry(root.into()).or_default().clone()
        };
        // Different workspaces do not block each other's discovery or stat checks.
        let mut snapshot = slot
            .lock()
            .expect("Git workspace discovery lock is healthy");
        if let Some(current) = snapshot.as_ref()
            && current.current()
        {
            return Ok(current.paths.clone());
        }
        #[cfg(test)]
        self.scans
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let discovered = discover(root)?;
        let paths = discovered.paths.clone();
        // Directory identities and timestamps detect ordinary metadata changes.
        // They do not provide isolation against writers restoring those witnesses.
        *snapshot = discovered.current().then_some(discovered);
        Ok(paths)
    }
}

#[cfg(test)]
mod tests;
