use super::{check, integrity, runtime};
use notify::EventKind;
#[cfg(not(target_os = "linux"))]
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
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

#[cfg(any(target_os = "linux", test))]
mod watchers;

enum Backend {
    #[cfg(not(target_os = "linux"))]
    Native(Option<RecommendedWatcher>),
    #[cfg(any(target_os = "linux", test))]
    Limited(watchers::Watch),
}

impl Backend {
    fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::Limited(watchers::Watch::new())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self::Native(None)
        }
    }

    fn active(&self) -> bool {
        match self {
            #[cfg(not(target_os = "linux"))]
            Self::Native(watcher) => watcher.is_some(),
            #[cfg(any(target_os = "linux", test))]
            Self::Limited(watcher) => watcher.active(),
        }
    }

    fn covers(&self, root: &Path) -> bool {
        #[cfg(not(any(target_os = "linux", test)))]
        let _ = root;
        match self {
            #[cfg(not(target_os = "linux"))]
            Self::Native(watcher) => watcher.is_some(),
            #[cfg(any(target_os = "linux", test))]
            Self::Limited(watcher) => watcher.covers(root),
        }
    }

    fn rebuilds_on_change(&self) -> bool {
        match self {
            #[cfg(not(target_os = "linux"))]
            Self::Native(_) => false,
            #[cfg(any(target_os = "linux", test))]
            Self::Limited(_) => true,
        }
    }

    fn watch(&mut self, root: &Path, handler: impl notify::EventHandler) {
        match self {
            #[cfg(not(target_os = "linux"))]
            Self::Native(backend) => {
                *backend = None;
                let Ok(mut watcher) =
                    RecommendedWatcher::new(handler, Config::default().with_follow_symlinks(false))
                else {
                    return;
                };
                if watcher.watch(root, RecursiveMode::Recursive).is_ok() {
                    *backend = Some(watcher);
                }
            }
            #[cfg(any(target_os = "linux", test))]
            Self::Limited(watcher) => watcher.watch(root, handler),
        }
    }
}

struct Verified {
    stamp: Vec<integrity::Stamp>,
    epoch: u64,
    checked: Instant,
}

pub(super) struct Validation {
    root: PathBuf,
    backend: Backend,
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
            backend: Backend::new(),
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
            Ok(integrity::verify(root, root, files, cache, cancel)?
                && integrity::caches(root, files, cancel)?)
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
        let watched = self.backend.active();
        if watched
            && !self.failed.load(Ordering::Acquire)
            && let Some(verified) = &self.verified
            && verified.checked.elapsed() < INTERVAL
            && verified.epoch == epoch
            && stamp.as_ref() == Some(&verified.stamp)
        {
            return Ok(true);
        }
        if !watched
            || self.failed.swap(false, Ordering::AcqRel)
            || (self.verified.is_none() && self.backend.rebuilds_on_change())
            || self.verified.as_ref().is_some_and(|verified| {
                stamp.as_ref() != Some(&verified.stamp)
                    || (self.backend.rebuilds_on_change() && verified.epoch != epoch)
            })
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
            && self.backend.covers(&self.root)
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
        let epoch = self.epoch.clone();
        let failed = self.failed.clone();
        #[cfg(test)]
        let changes = self.changes.clone();
        self.backend
            .watch(&self.root, move |event: notify::Result<notify::Event>| {
                if let Ok(event) = &event
                    && !event.need_rescan()
                    && (matches!(event.kind, EventKind::Access(_))
                        || event.paths.iter().all(|path| {
                            path.file_name().is_some_and(|name| name == ".sailry-used")
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
            });
    }
}

#[cfg(test)]
mod tests;
