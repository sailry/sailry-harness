//! Lazy, rebuildable worktree structure. Parsing never holds a database writer.
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, NodeId, WorktreeId, code_index::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::sync::{OnceCell, Semaphore};

mod graph;
mod index;
mod parser;
mod queries;
mod resources;
mod sync;
pub(crate) use queries::Queries;
#[cfg(test)]
mod tests;

const MAX_FILES: usize = 50_000;
const BATCH: usize = 16;
/// Shared blocking read and parse workers across all worktrees.
const READERS: usize = 4;
const MAX_ISSUES: usize = 5;

pub(crate) struct Service {
    profile: Option<PathBuf>,
    temporary: Mutex<Option<tempfile::TempDir>>,
    enabled: std::sync::atomic::AtomicBool,
    node: NodeId,
    closed: CancellationToken,
    watches: Arc<crate::files::watch::Watches>,
    resources: OnceCell<Arc<resources::Resources>>,
    parsing: Arc<Semaphore>,
    worktrees: Mutex<BTreeMap<WorktreeId, Arc<Worktree>>>,
}

struct Worktree {
    root: PathBuf,
    database: PathBuf,
    index: OnceCell<index::Index>,
    sync: tokio::sync::Mutex<sync::Progress>,
    access: tokio::sync::RwLock<()>,
    dirty: Mutex<Dirty>,
    stop: CancellationToken,
    observed: OnceCell<()>,
    watcher: Mutex<Option<tokio::task::JoinHandle<()>>>,
    #[cfg(test)]
    prepared: Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
}

#[derive(Default)]
struct Dirty {
    generation: u64,
    paths: BTreeMap<String, u64>,
    watch_failed: bool,
}

impl Dirty {
    fn change(&mut self, path: Option<&str>) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(path) = path {
            self.paths.insert(path.into(), self.generation);
        }
    }
    fn finish(&mut self, generation: u64) -> bool {
        if self.generation != generation {
            return false;
        }
        self.paths.clear();
        true
    }
}

impl Service {
    pub(crate) fn new(
        profile: Option<PathBuf>,
        node: NodeId,
        watches: Arc<crate::files::watch::Watches>,
        closed: CancellationToken,
    ) -> Arc<Self> {
        Arc::new(Self {
            profile,
            temporary: Default::default(),
            enabled: std::sync::atomic::AtomicBool::new(true),
            node,
            watches,
            closed,
            resources: OnceCell::new(),
            parsing: Arc::new(Semaphore::new(READERS)),
            worktrees: Mutex::new(BTreeMap::new()),
        })
    }

    async fn resources(&self) -> Result<Arc<resources::Resources>, Fault> {
        self.resources
            .get_or_try_init(|| async { resources::Resources::bundled().map(Arc::new) })
            .await
            .cloned()
    }

    fn worktree(&self, id: WorktreeId, root: PathBuf) -> Result<Arc<Worktree>, Fault> {
        let mut worktrees = self.worktrees.lock().expect("code index lock is healthy");
        check(&self.closed)?;
        if !self.enabled.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "code index is disabled",
            ));
        }
        if let Some(worktree) = worktrees.get(&id) {
            if worktree.root != root {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "worktree root changed",
                ));
            }
            return Ok(worktree.clone());
        }
        let profile = match &self.profile {
            Some(profile) => profile.clone(),
            None => {
                let mut temporary = self.temporary.lock().unwrap();
                if temporary.is_none() {
                    *temporary = Some(tempfile::tempdir().map_err(error)?);
                }
                temporary.as_ref().unwrap().path().to_owned()
            }
        };
        let worktree = Arc::new(Worktree {
            root: root.clone(),
            database: profile
                .join("cache/code-index")
                .join(id.to_string())
                .join("index.turso"),
            index: OnceCell::new(),
            sync: Default::default(),
            access: Default::default(),
            dirty: Default::default(),
            stop: self.closed.child_token(),
            observed: OnceCell::new(),
            watcher: Default::default(),
            #[cfg(test)]
            prepared: Default::default(),
        });
        worktrees.insert(id, worktree.clone());
        Ok(worktree)
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn configure(&self, enabled: bool) {
        let mut worktrees = self.worktrees.lock().expect("code index lock is healthy");
        self.enabled
            .store(enabled, std::sync::atomic::Ordering::Release);
        if !enabled {
            for worktree in worktrees.values() {
                worktree.stop.cancel();
            }
            worktrees.clear();
        }
    }

    pub(crate) fn changed(&self, worktree: WorktreeId, path: Option<&str>) {
        if let Some(worktree) = self.worktrees.lock().unwrap().get(&worktree) {
            worktree.dirty.lock().unwrap().change(path);
        }
    }

    pub(crate) fn invalidate(&self, root: &std::path::Path, path: Option<&str>) {
        for worktree in self
            .worktrees
            .lock()
            .unwrap()
            .values()
            .filter(|worktree| worktree.root == root)
        {
            worktree.dirty.lock().unwrap().change(path);
        }
    }

    pub(crate) async fn remove(&self, id: WorktreeId) -> Result<(), Fault> {
        let worktree = self.worktrees.lock().unwrap().get(&id).cloned();
        let directory = if let Some(worktree) = &worktree {
            worktree.stop.cancel();
            // Readers release initialization and native leases before cache removal.
            let _access = worktree.access.write().await;
            if let Some(index) = worktree.index.get() {
                index.close().await?;
            }
            worktree.database.parent().unwrap().to_owned()
        } else {
            let profile = self.profile.clone().or_else(|| {
                self.temporary
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|directory| directory.path().to_owned())
            });
            let Some(profile) = profile else {
                return Ok(());
            };
            profile.join("cache/code-index").join(id.to_string())
        };
        match tokio::fs::remove_dir_all(directory).await {
            Ok(()) => {}
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {}
            Err(cause) => return Err(error(cause)),
        }
        self.worktrees.lock().unwrap().remove(&id);
        Ok(())
    }

    pub(crate) fn shutdown(&self) {
        self.configure(false);
    }

    pub(crate) async fn outline(
        &self,
        id: WorktreeId,
        root: PathBuf,
        path: &str,
        stop: &CancellationToken,
    ) -> Result<Outline, Fault> {
        crate::files::path::entry_components(path)?;
        let worktree = self.worktree(id, root)?;
        let running = stop.child_token();
        let _cancel = running.clone().drop_guard();
        let stop = &running;
        let operation = async {
            let _access = worktree.access.read().await;
            check(&worktree.stop)?;
            let mut progress = tokio::select! {biased; _=stop.cancelled()=>return Err(cancelled()),guard=worktree.sync.lock()=>guard};
            worktree
                .observe(self.watches.clone(), self.node, id, stop)
                .await?;
            check(stop)?;
            let index = worktree.index().await?;
            let resources = self.resources().await?;
            let directory = sync::open(&worktree).await?;
            let prior = sync::prior(progress.cached(index, path).await?);
            let file = self
                .read(
                    &worktree,
                    directory,
                    vec![(path.to_owned(), prior)],
                    resources.clone(),
                    stop,
                    Instant::now() + Duration::from_secs(10),
                )
                .await?
                .pop()
                .expect("one file was read")?;
            if let Some(file) = file
                && let Some(revision) = index
                    .replace(std::slice::from_ref(&file), &resources.fingerprint, stop)
                    .await?
            {
                progress.replaced(&file, &resources.fingerprint, revision);
            }
            let mut outline = index.outline(path).await?;
            outline.partial |= worktree.dirty.lock().unwrap().watch_failed;
            Ok(outline)
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(cancelled()),
            _ = worktree.stop.cancelled() => Err(cancelled()),
            result = operation => result,
        }
    }

    pub(crate) async fn search(
        &self,
        id: WorktreeId,
        root: PathBuf,
        query: &Search,
        stop: &CancellationToken,
    ) -> Result<SearchPage, Fault> {
        if query.limit == 0
            || query.query.len() > 4096
            || query.name.as_ref().is_some_and(|name| name.len() > 512)
            || query
                .language
                .as_ref()
                .is_some_and(|language| language.len() > 128)
        {
            return Err(Fault::invalid("invalid code search limits"));
        }
        if let Some(path) = &query.path {
            crate::files::path::entry_components(path)?;
        }
        let worktree = self.worktree(id, root)?;
        let running = stop.child_token();
        let _cancel = running.clone().drop_guard();
        let stop = &running;
        let operation = async {
            let _access = worktree.access.read().await;
            check(&worktree.stop)?;
            worktree
                .observe(self.watches.clone(), self.node, id, stop)
                .await?;
            let index = worktree.index().await?;
            let mut held = worktree.sync.try_lock().ok();
            let report = if let Some(progress) = held.as_mut() {
                self.synchronize(
                    &worktree,
                    index,
                    self.resources().await?,
                    stop,
                    progress,
                    sync::Limits::default(),
                )
                .await?
            } else {
                // Another reader is already refreshing this worktree. Its committed
                // transactions remain readable without queuing a second full scan.
                sync::Report::interrupted()
            };
            let scoped = report.scoped(query.path.as_deref());
            // Watch events carry no paths, so a bounded scan can still return
            // files changed outside the Agent. Matched files are re-read before
            // their revisions reach revision-checked line reads.
            let mut page = if let Some(mut progress) = held {
                self.validated_search(&worktree, index, &mut progress, query, stop)
                    .await?
            } else {
                index.search(query, stop).await?
            };
            page.visited = report.visited;
            page.skipped = scoped.skipped;
            page.partial |= scoped.partial;
            page.truncated |= scoped.truncated;
            page.issues = scoped.issues;
            Ok(page)
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(cancelled()),
            _ = worktree.stop.cancelled() => Err(cancelled()),
            result = operation => result,
        }
    }

    async fn validated_search(
        &self,
        worktree: &Arc<Worktree>,
        index: &index::Index,
        progress: &mut sync::Progress,
        query: &Search,
        stop: &CancellationToken,
    ) -> Result<SearchPage, Fault> {
        let mut page = index.search(query, stop).await?;
        let mut checked = BTreeSet::new();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let paths: BTreeSet<_> = page
                .symbols
                .iter()
                .map(|symbol| symbol.path.clone())
                .filter(|path| !checked.contains(path))
                .collect();
            if paths.is_empty() {
                break;
            }
            let refreshed = self
                .refresh(worktree, index, progress, paths, stop, deadline)
                .await?;
            let stalled = refreshed.checked.is_empty();
            checked.extend(refreshed.checked);
            if refreshed.changed {
                page = index.search(query, stop).await?;
            }
            if stalled || Instant::now() >= deadline {
                page.symbols.retain(|symbol| checked.contains(&symbol.path));
                page.partial = true;
                page.truncated = true;
                break;
            }
        }
        Ok(page)
    }
}

impl Worktree {
    async fn observe(
        self: &Arc<Self>,
        watches: Arc<crate::files::watch::Watches>,
        node: NodeId,
        id: WorktreeId,
        stop: &CancellationToken,
    ) -> Result<(), Fault> {
        let observe = self.observed.get_or_init(|| async {
            let root = self.root.clone();
            let closed = self.stop.clone();
            // Backend creation can enumerate directories. Reuse the existing per-root
            // watcher outside the async worker and the service's registration lock.
            let events = tokio::task::spawn_blocking(move || {
                watches.subscribe(root, node, id, false, closed)
            })
            .await;
            match events {
                Ok(Ok(mut events)) => {
                    let weak = Arc::downgrade(self);
                    *self.watcher.lock().unwrap() = Some(tokio::spawn(async move {
                        let mut initial = true;
                        loop {
                            let result = events.next().await;
                            let Some(worktree) = Weak::upgrade(&weak) else {
                                break;
                            };
                            if worktree.stop.is_cancelled() {
                                break;
                            }
                            let mut dirty = worktree.dirty.lock().unwrap();
                            if result.is_err() {
                                dirty.watch_failed = true;
                                dirty.change(None);
                                break;
                            }
                            if initial {
                                initial = false;
                            } else {
                                dirty.change(None);
                            }
                        }
                    }));
                }
                _ => self.dirty.lock().unwrap().watch_failed = true,
            }
        });
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(cancelled()),
            _ = self.stop.cancelled() => Err(cancelled()),
            _ = observe => Ok(()),
        }
    }

    async fn index(&self) -> Result<&index::Index, Fault> {
        self.index
            .get_or_try_init(|| async {
                let directory = self
                    .database
                    .parent()
                    .expect("index has a worktree directory");
                let cache = directory.parent().expect("index has a cache directory");
                crate::profile::private_directory(
                    cache.parent().expect("cache has a profile directory"),
                )
                .map_err(error)?;
                crate::profile::private_directory(cache).map_err(error)?;
                crate::profile::private_directory(directory).map_err(error)?;
                index::Index::open(&self.database).await
            })
            .await
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.watcher.get_mut().unwrap().take() {
            task.abort();
        }
    }
}

fn error(error: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Internal, format!("code index: {error}"))
}
fn cancelled() -> Fault {
    Fault::new(ErrorCode::Cancelled, "code query was cancelled")
}
fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}
