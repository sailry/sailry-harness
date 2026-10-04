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
pub(crate) struct Watches(Mutex<State>);

#[derive(Default)]
struct State {
    closed: bool,
    roots: BTreeMap<PathBuf, Weak<Active>>,
}

struct Active {
    backend: Mutex<Option<RecommendedWatcher>>,
    changes: watch::Sender<Result<(), Fault>>,
}

impl Watches {
    pub(crate) fn remove(&self, root: &Path) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(active) = state.roots.remove(root).and_then(|watch| watch.upgrade()) {
            let _ = active.changes.send_replace(Err(unavailable()));
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
            let sink = changes.clone();
            let mut backend = RecommendedWatcher::new(
                move |event: notify::Result<notify::Event>| {
                    if matches!(&event, Ok(event) if matches!(event.kind, EventKind::Access(_)) && !event.need_rescan()) {
                        return;
                    }
                    // A backend rescan, queue coalescing, or ordinary event all invalidate
                    // the loaded view. Errors remain terminal instead of being overwritten.
                    sink.send_if_modified(|state| {
                        if state.is_err() { return false; }
                        *state = event.map(|_| ()).map_err(|_| unavailable());
                        true
                    });
                },
                Config::default().with_follow_symlinks(false),
            ).map_err(|_| unavailable())?;
            backend
                .watch(&root, RecursiveMode::Recursive)
                .map_err(|_| unavailable())?;
            let active = Arc::new(Active {
                backend: Mutex::new(Some(backend)),
                changes,
            });
            state.roots.insert(root, Arc::downgrade(&active));
            active
        };
        Ok(Box::new(Events {
            changes: active.changes.subscribe(),
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
            active
                .backend
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take();
        }
        state.roots.clear();
    }
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
                watches.subscribe(root, NodeId([1; 32]), WorktreeId::new(), cancelled)
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
        let (changes, receiver) = watch::channel(Ok(()));
        let active = Arc::new(Active {
            backend: Mutex::new(None),
            changes,
        });
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
        for _ in 0..10_000 {
            let _ = active.changes.send_replace(Ok(()));
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
            .subscribe(root.clone(), node, worktree, CancellationToken::new())
            .unwrap();
        let second = watches
            .subscribe(root.clone(), node, worktree, CancellationToken::new())
            .unwrap();
        let active = watches.0.lock().unwrap().roots[&root].upgrade().unwrap();
        assert_eq!(Arc::strong_count(&active), 3);
        first.next().await.unwrap();
        drop(second);
        assert_eq!(Arc::strong_count(&active), 2);
        watches.shutdown();
        assert!(active.backend.lock().unwrap().is_none());
        assert_eq!(first.next().await.unwrap_err().code, ErrorCode::Unavailable);
        assert!(
            watches
                .subscribe(root, node, worktree, CancellationToken::new())
                .is_err()
        );
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
                CancellationToken::new(),
            )
            .unwrap();
        let active = watches.0.lock().unwrap().roots[&root].clone();
        drop(events);
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
            .subscribe(root.clone(), node, worktree, CancellationToken::new())
            .unwrap();
        let active = watches.0.lock().unwrap().roots[&root].upgrade().unwrap();
        events.next().await.unwrap();
        watches.remove(&root);
        assert!(active.backend.lock().unwrap().is_none());
        assert_eq!(
            events.next().await.unwrap_err().code,
            ErrorCode::Unavailable
        );
        assert!(watches.0.lock().unwrap().roots.is_empty());
        // A failed removal can establish a fresh observer on the still-existing directory.
        watches
            .subscribe(root, node, worktree, CancellationToken::new())
            .unwrap();
    }
}
