//! Watch lifecycle references: old Platform 727ce0a harbor-file-service/src/watch.rs
//! and Code 67ae9fa0 worktree_file_watch.rs (Apache-2.0).
//! Coalesce invalidations rather than retaining raw paths or another filesystem index.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use sailry_link::{CancellationToken, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, NodeId, Update, WorktreeId};
use tokio::sync::watch;

#[derive(Default)]
pub(crate) struct Watches(Mutex<State>, Option<crate::git::Git>);

#[derive(Default)]
struct State {
    closed: bool,
    roots: BTreeMap<PathBuf, Weak<Active>>,
}

struct Active {
    backend: Mutex<Option<RecommendedWatcher>>,
    changes: watch::Sender<Result<(), Fault>>,
    repository: watch::Sender<Result<(), Fault>>,
    filter: Arc<Mutex<crate::git::WatchFilter>>,
}

impl Watches {
    pub(crate) fn new(git: crate::git::Git) -> Self {
        Self(Mutex::default(), Some(git))
    }

    /// Invalidate existing observers after a managed filesystem effect.
    pub(crate) fn changed(&self, root: &Path) {
        if let Some(git) = &self.1 {
            git.invalidate(root);
        }
        let active = {
            let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            if state.closed {
                return;
            }
            state.roots.get(root).and_then(Weak::upgrade)
        };
        if let Some(active) = active {
            invalidate(&active.changes, Ok(()));
            invalidate(&active.repository, Ok(()));
        }
    }

    /// File views observe every managed effect; repository views observe relevant paths.
    pub(crate) fn changed_paths(&self, root: &Path, paths: &[PathBuf]) {
        let active = {
            let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            if state.closed {
                return;
            }
            state.roots.get(root).and_then(Weak::upgrade)
        };
        if let Some(active) = active {
            invalidate(&active.changes, Ok(()));
            if active
                .filter
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .changed(paths)
            {
                if let Some(git) = &self.1 {
                    git.invalidate(root);
                }
                invalidate(&active.repository, Ok(()));
            }
        } else if let Some(git) = &self.1 {
            git.invalidate(root);
        }
    }

    pub(crate) fn remove(&self, root: &Path) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(active) = state.roots.remove(root).and_then(|watch| watch.upgrade()) {
            let _ = active.changes.send_replace(Err(unavailable()));
            let _ = active.repository.send_replace(Err(unavailable()));
            active
                .backend
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
        }
    }

    pub(crate) fn subscribe(
        &self,
        root: PathBuf,
        node: NodeId,
        worktree: WorktreeId,
        repository: bool,
        closed: CancellationToken,
    ) -> Result<Box<dyn Subscription>, Fault> {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed || closed.is_cancelled() {
            return Err(unavailable());
        }
        state.roots.retain(|_, watch| watch.strong_count() > 0);
        let active = if let Some(active) = state.roots.get(&root).and_then(Weak::upgrade) {
            active
        } else {
            if state.roots.len() >= 32 {
                return Err(Fault::new(ErrorCode::Busy, "file watch capacity exhausted"));
            }
            // Roots come from Node registration, not raw paths sent by a controller.
            let _directory = super::path::root(&root)?;
            let (changes, _) = watch::channel(Ok(()));
            let (repository, _) = watch::channel(Ok(()));
            let sink = changes.clone();
            let repository_sink = repository.clone();
            let filter = Arc::new(Mutex::new(crate::git::Git::watch_filter(&root)?));
            let changed_filter = filter.clone();
            let git = self.1.clone();
            let watched_root = root.clone();
            let mut backend = RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| {
                    if matches!(&event, Ok(event) if matches!(event.kind, EventKind::Access(_)) && !event.need_rescan()) {
                        return;
                    }
                    let relevant = match &event {
                        Ok(event) if !event.need_rescan() => changed_filter
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .changed(&event.paths),
                        _ => true,
                    };
                    if relevant {
                        if let Some(git) = &git {
                            git.invalidate(&watched_root);
                        }
                        invalidate(&repository_sink, event.as_ref().map(|_| ()).map_err(|_| unavailable()));
                    }
                    // A backend rescan, queue coalescing, or ordinary event all invalidate
                    // the loaded view. Errors remain terminal instead of being overwritten.
                    invalidate(&sink, event.map(|_| ()).map_err(|_| unavailable()));
                },
                Config::default().with_follow_symlinks(false),
            ).map_err(|_| unavailable())?;
            backend
                .watch(&root, RecursiveMode::Recursive)
                .map_err(|_| unavailable())?;
            for metadata in crate::git::Git::watch_roots(&root)? {
                if !metadata.starts_with(&root) {
                    backend
                        .watch(&metadata, RecursiveMode::Recursive)
                        .map_err(|_| unavailable())?;
                }
            }
            if let Some(git) = &self.1 {
                git.invalidate(&root);
            }
            let active = Arc::new(Active {
                backend: Mutex::new(Some(backend)),
                changes,
                repository,
                filter,
            });
            state.roots.insert(root, Arc::downgrade(&active));
            active
        };
        Ok(Box::new(Events {
            changes: if repository {
                active.repository.subscribe()
            } else {
                active.changes.subscribe()
            },
            _active: active,
            node,
            worktree,
            initial: true,
            pending: None,
            closed,
        }))
    }

    pub(crate) fn shutdown(&self) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        for active in state.roots.values().filter_map(Weak::upgrade) {
            let _ = active.changes.send_replace(Err(unavailable()));
            let _ = active.repository.send_replace(Err(unavailable()));
            active
                .backend
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
        }
        state.roots.clear();
    }
}

fn invalidate(changes: &watch::Sender<Result<(), Fault>>, result: Result<(), Fault>) {
    changes.send_if_modified(|state| {
        if state.is_err() {
            return false;
        }
        *state = result;
        true
    });
}

struct Events {
    changes: watch::Receiver<Result<(), Fault>>,
    _active: Arc<Active>,
    node: NodeId,
    worktree: WorktreeId,
    initial: bool,
    pending: Option<tokio::time::Instant>,
    closed: CancellationToken,
}

impl Subscription for Events {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if self.closed.is_cancelled() {
                return Err(unavailable());
            }
            if !self.initial {
                if self.pending.is_none() {
                    tokio::select! {
                        biased;
                        _ = self.closed.cancelled() => return Err(unavailable()),
                        changed = self.changes.changed() => changed.map_err(|_| unavailable())?,
                    }
                    self.pending = Some(tokio::time::Instant::now() + Duration::from_millis(200));
                }
                tokio::select! {
                    biased;
                    _ = self.closed.cancelled() => return Err(unavailable()),
                    _ = tokio::time::sleep_until(self.pending.unwrap()) => {},
                }
            }
            self.initial = false;
            self.pending = None;
            self.changes.borrow_and_update().clone()?;
            Ok(Update::FilesChanged {
                node: self.node,
                worktree: self.worktree,
            })
        })
    }
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "file watch is unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancels_before_backend_creation() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let watches = Arc::new(Watches::default());
        let cancelled = CancellationToken::new();
        let guard = cancelled.clone().drop_guard();
        let locked = watches.0.lock().unwrap();
        let (started, waiting) = std::sync::mpsc::channel();
        let worker = {
            let watches = watches.clone();
            std::thread::spawn(move || {
                started.send(()).unwrap();
                watches.subscribe(root, NodeId([1; 32]), WorktreeId::new(), false, cancelled)
            })
        };
        waiting.recv().unwrap();
        // A preparation waiting behind another setup is cancelled with its caller.
        drop(guard);
        drop(locked);
        assert!(matches!(
            worker.join().unwrap(),
            Err(Fault {
                code: ErrorCode::Unavailable,
                ..
            })
        ));
        assert!(watches.0.lock().unwrap().roots.is_empty());
    }

    #[tokio::test]
    async fn preserves_pending_invalidation() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let (changes, receiver) = watch::channel(Ok(()));
        let active = Arc::new(Active {
            backend: Mutex::new(None),
            changes,
            repository: watch::channel(Ok(())).0,
            filter: Arc::new(Mutex::new(crate::git::Git::watch_filter(&root).unwrap())),
        });
        let watches = Watches::default();
        watches
            .0
            .lock()
            .unwrap()
            .roots
            .insert(root.clone(), Arc::downgrade(&active));
        let mut events = Events {
            changes: receiver,
            _active: active.clone(),
            node: NodeId([1; 32]),
            worktree: WorktreeId::new(),
            initial: true,
            pending: None,
            closed: CancellationToken::new(),
        };
        events.next().await.unwrap();
        watches.changed(Path::new("other"));
        assert!(!events.changes.has_changed().unwrap());
        for _ in 0..10_000 {
            watches.changed(&root);
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(20), events.next())
                .await
                .is_err()
        );
        assert!(events.pending.is_some());
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), events.next())
                .await
                .unwrap()
                .unwrap(),
            Update::FilesChanged { .. }
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), events.next())
                .await
                .is_err()
        );
        let _ = active.changes.send_replace(Err(unavailable()));
        watches.changed(&root);
        assert_eq!(
            events.next().await.unwrap_err().code,
            ErrorCode::Unavailable
        );
    }

    #[tokio::test]
    async fn shares_backend_until_shutdown() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let watches = Watches::default();
        let node = NodeId([1; 32]);
        let worktree = WorktreeId::new();
        let mut first = watches
            .subscribe(
                root.clone(),
                node,
                worktree,
                false,
                CancellationToken::new(),
            )
            .unwrap();
        let second = watches
            .subscribe(root.clone(), node, worktree, true, CancellationToken::new())
            .unwrap();
        let active = watches.0.lock().unwrap().roots[&root].upgrade().unwrap();
        assert_eq!(Arc::strong_count(&active), 3);
        first.next().await.unwrap();
        drop(second);
        assert_eq!(Arc::strong_count(&active), 2);
        watches.shutdown();
        watches.changed(&root);
        assert!(active.backend.lock().unwrap().is_none());
        assert_eq!(first.next().await.unwrap_err().code, ErrorCode::Unavailable);
        assert!(
            watches
                .subscribe(root, node, worktree, false, CancellationToken::new())
                .is_err()
        );
    }

    #[test]
    fn filters_repository_effects() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        git2::Repository::init(&root).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("source.txt"), "source").unwrap();
        let (changes, mut files) = watch::channel(Ok(()));
        let (repository, mut git) = watch::channel(Ok(()));
        let active = Arc::new(Active {
            backend: Mutex::new(None),
            changes,
            repository,
            filter: Arc::new(Mutex::new(crate::git::Git::watch_filter(&root).unwrap())),
        });
        let watches = Watches::default();
        watches
            .0
            .lock()
            .unwrap()
            .roots
            .insert(root.clone(), Arc::downgrade(&active));

        watches.changed_paths(&root, &[root.join("target/build.log")]);
        assert!(files.has_changed().unwrap());
        assert!(!git.has_changed().unwrap());
        files.borrow_and_update();
        watches.changed_paths(&root, &[root.join("source.txt")]);
        assert!(files.has_changed().unwrap());
        assert!(git.has_changed().unwrap());
        files.borrow_and_update();
        git.borrow_and_update();
        watches.changed_paths(Path::new("other"), &[root.join("source.txt")]);
        assert!(!files.has_changed().unwrap());
        assert!(!git.has_changed().unwrap());
        watches.changed(&root);
        assert!(files.has_changed().unwrap());
        assert!(git.has_changed().unwrap());
    }

    #[test]
    fn last_observer_releases_backend() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let watches = Watches::default();
        let events = watches
            .subscribe(
                root.clone(),
                NodeId([1; 32]),
                WorktreeId::new(),
                false,
                CancellationToken::new(),
            )
            .unwrap();
        let active = watches.0.lock().unwrap().roots[&root].clone();
        drop(events);
        watches.changed(&root);
        assert!(active.upgrade().is_none());
    }

    #[tokio::test]
    async fn removal_releases_backend() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let watches = Watches::default();
        let node = NodeId([1; 32]);
        let worktree = WorktreeId::new();
        let mut events = watches
            .subscribe(
                root.clone(),
                node,
                worktree,
                false,
                CancellationToken::new(),
            )
            .unwrap();
        let active = watches.0.lock().unwrap().roots[&root].upgrade().unwrap();
        events.next().await.unwrap();
        watches.remove(&root);
        watches.changed(&root);
        assert!(active.backend.lock().unwrap().is_none());
        assert_eq!(
            events.next().await.unwrap_err().code,
            ErrorCode::Unavailable
        );
        assert!(watches.0.lock().unwrap().roots.is_empty());
        // A failed removal can establish a fresh observer on the still-existing directory.
        watches
            .subscribe(root, node, worktree, false, CancellationToken::new())
            .unwrap();
    }
}
