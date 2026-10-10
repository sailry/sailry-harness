use super::{check, integrity, runtime};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sailry_link::CancellationToken;
use sailry_protocol::Fault;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

// Filesystem events invalidate hot entries. Revalidation also bounds missed
// events; an unavailable watcher uses full validation on each access instead.
const INTERVAL: Duration = Duration::from_secs(5 * 60);

struct Verified {
    stamp: Vec<integrity::Stamp>,
    epoch: u64,
    checked: Instant,
}

pub(super) struct Validation {
    root: PathBuf,
    backend: Option<RecommendedWatcher>,
    epoch: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
    #[cfg(test)]
    changes: tokio::sync::watch::Sender<()>,
    verified: Option<Verified>,
    cache: integrity::Cache,
}

impl Validation {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            root,
            backend: None,
            epoch: Arc::default(),
            failed: Arc::default(),
            #[cfg(test)]
            changes: tokio::sync::watch::channel(()).0,
            verified: None,
            cache: integrity::Cache::default(),
        }
    }

    pub(super) fn runtime(
        &mut self,
        python: &Path,
        receipt: &str,
        files: &[integrity::File],
        cancel: &CancellationToken,
    ) -> Result<bool, Fault> {
        let paths = [self.root.join("python-runtime.json"), python.to_owned()];
        self.validate(&paths, cancel, |root, cache| {
            let path = root.join("python-runtime.json");
            let Ok(metadata) = path.symlink_metadata() else {
                return Ok(false);
            };
            if !metadata.is_file() || metadata.is_symlink() || metadata.len() > 16 * 1024 * 1024 {
                return Ok(false);
            }
            let content = std::fs::read(path).map_err(super::storage)?;
            if blake3::hash(&content).to_hex().as_str() != receipt {
                return Ok(false);
            }
            integrity::verify(root, root, files, cache, cancel)
        })
    }

    pub(super) fn environment(
        &mut self,
        base: &Path,
        key: &str,
        cancel: &CancellationToken,
    ) -> Result<bool, Fault> {
        let paths = [
            self.root.join(integrity::MARKER),
            runtime::python(&self.root),
        ];
        self.validate(&paths, cancel, |root, cache| {
            integrity::valid(root, base, key, cache, cancel)
        })
    }

    #[cfg(test)]
    pub(super) fn subscribe(&self) -> tokio::sync::watch::Receiver<()> {
        self.changes.subscribe()
    }

    fn validate(
        &mut self,
        paths: &[PathBuf],
        cancel: &CancellationToken,
        verify: impl FnOnce(&Path, &mut integrity::Cache) -> Result<bool, Fault>,
    ) -> Result<bool, Fault> {
        check(cancel)?;
        let stamp = self.stamp(paths);
        let epoch = self.epoch.load(Ordering::Acquire);
        if self.backend.is_some()
            && !self.failed.load(Ordering::Acquire)
            && let Some(verified) = &self.verified
            && verified.checked.elapsed() < INTERVAL
            && verified.epoch == epoch
            && stamp.as_ref() == Some(&verified.stamp)
        {
            return Ok(true);
        }
        if self.backend.is_none()
            || self.failed.swap(false, Ordering::AcqRel)
            || self
                .verified
                .as_ref()
                .is_some_and(|verified| stamp.as_ref() != Some(&verified.stamp))
        {
            self.watch();
        }
        self.verified = None;
        let epoch = self.epoch.load(Ordering::Acquire);
        if !verify(&self.root, &mut self.cache)? {
            return Ok(false);
        }
        // Root, receipt and interpreter replacement remain synchronous checks.
        // An event arriving during validation prevents this entry staying hot.
        if let Some(current) = self.stamp(paths)
            && stamp.as_ref() == Some(&current)
        {
            self.verified = Some(Verified {
                stamp: current,
                epoch,
                checked: Instant::now(),
            });
        }
        check(cancel)?;
        Ok(true)
    }

    fn stamp(&self, paths: &[PathBuf]) -> Option<Vec<integrity::Stamp>> {
        let root = self.root.symlink_metadata().ok()?;
        if !root.is_dir() || root.is_symlink() {
            return None;
        }
        let mut result = vec![integrity::Stamp::new(&root)];
        for path in paths {
            let metadata = path.symlink_metadata().ok()?;
            if !metadata.is_file() || metadata.is_symlink() {
                return None;
            }
            result.push(integrity::Stamp::new(&metadata));
        }
        Some(result)
    }

    fn watch(&mut self) {
        self.backend = None;
        let epoch = self.epoch.clone();
        let failed = self.failed.clone();
        #[cfg(test)]
        let changes = self.changes.clone();
        let mut watcher = match RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                if let Ok(event) = &event
                    && !event.need_rescan()
                    && (matches!(event.kind, EventKind::Access(_))
                        || event.paths.iter().all(|path| {
                            path.file_name().is_some_and(|name| name == ".sailry-used")
                                || path
                                    .components()
                                    .any(|part| part.as_os_str() == "__pycache__")
                        }))
                {
                    return;
                }
                if event.is_err() {
                    failed.store(true, Ordering::Release);
                }
                epoch.fetch_add(1, Ordering::AcqRel);
                #[cfg(test)]
                changes.send_replace(());
            },
            Config::default().with_follow_symlinks(false),
        ) {
            Ok(watcher) => watcher,
            Err(_) => return,
        };
        if watcher.watch(&self.root, RecursiveMode::Recursive).is_ok() {
            self.backend = Some(watcher);
        }
    }
}

#[cfg(test)]
mod tests;
