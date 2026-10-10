use super::{INTERVAL, integrity};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::Instant,
};

// Linux allocates an inotify subscription for each directory. Explicit watches
// cannot grow behind this process-wide budget when an environment creates files.
// The pool retains at most 16 native watchers; notify retires their threads and
// descriptors asynchronously after subscriptions have been removed.
#[cfg(target_os = "linux")]
const DIRECTORIES: usize = 2048;
#[cfg(target_os = "linux")]
const WATCHERS: usize = 16;

#[cfg(target_os = "linux")]
static POOL: std::sync::LazyLock<Arc<Mutex<Pool>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(Pool::new(DIRECTORIES, WATCHERS))));

type Coverage = Vec<(PathBuf, integrity::Stamp)>;

pub(super) struct Watch {
    state: Arc<Mutex<State>>,
    pool: Arc<Mutex<Pool>>,
}

#[derive(Default)]
struct State {
    backend: Option<RecommendedWatcher>,
    coverage: Coverage,
    rejected: Option<(Instant, Option<integrity::Stamp>)>,
}

impl State {
    fn clear(&mut self) {
        if let Some(mut backend) = self.backend.take() {
            // notify's Linux Drop sends asynchronous shutdown. Explicit unwatch
            // releases subscriptions before their budget can be reassigned.
            for (path, _) in self.coverage.iter().rev() {
                let _ = backend.unwatch(path);
            }
        }
        self.coverage.clear();
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.clear();
    }
}

struct Pool {
    directories: usize,
    watchers: usize,
    entries: VecDeque<Weak<Mutex<State>>>,
}

impl Pool {
    fn new(directories: usize, watchers: usize) -> Self {
        assert!(directories > 0 && watchers > 0);
        Self {
            directories,
            watchers,
            entries: VecDeque::new(),
        }
    }

    fn remove(&mut self, state: &Arc<Mutex<State>>) {
        self.entries
            .retain(|entry| entry.strong_count() > 0 && !entry.ptr_eq(&Arc::downgrade(state)));
    }

    fn reserve(&mut self, count: usize) {
        loop {
            let total = self
                .entries
                .iter()
                .filter_map(Weak::upgrade)
                .map(|entry| {
                    entry
                        .lock()
                        .expect("Python watcher lock is valid")
                        .coverage
                        .len()
                })
                .sum::<usize>();
            if self.entries.len() < self.watchers && total + count <= self.directories {
                break;
            }
            let Some(entry) = self.entries.pop_front().and_then(|entry| entry.upgrade()) else {
                continue;
            };
            let mut state = entry.lock().expect("Python watcher lock is valid");
            state.clear();
        }
    }
}

impl Watch {
    #[cfg(target_os = "linux")]
    pub(super) fn new() -> Self {
        Self::with_pool(POOL.clone())
    }

    #[cfg(test)]
    pub(super) fn isolated(directories: usize, watchers: usize) -> Self {
        Self::with_pool(Arc::new(Mutex::new(Pool::new(directories, watchers))))
    }

    #[cfg(test)]
    pub(super) fn peer(&self) -> Self {
        Self::with_pool(self.pool.clone())
    }

    fn with_pool(pool: Arc<Mutex<Pool>>) -> Self {
        Self {
            state: Arc::default(),
            pool,
        }
    }

    pub(super) fn active(&self) -> bool {
        let mut pool = self.pool.lock().expect("Python watcher pool lock is valid");
        pool.remove(&self.state);
        if self
            .state
            .lock()
            .expect("Python watcher lock is valid")
            .backend
            .is_none()
        {
            return false;
        }
        pool.entries.push_back(Arc::downgrade(&self.state));
        true
    }

    pub(super) fn covers(&self, root: &Path) -> bool {
        let previous = {
            let state = self.state.lock().expect("Python watcher lock is valid");
            state.backend.as_ref().map(|_| state.coverage.clone())
        };
        let Some(previous) = previous else {
            return false;
        };
        if coverage(root, previous.len()).as_ref() != Some(&previous) {
            return false;
        }
        let state = self.state.lock().expect("Python watcher lock is valid");
        state.backend.is_some() && state.coverage == previous
    }

    pub(super) fn watch(&self, root: &Path, handler: impl notify::EventHandler) {
        let stamp = root
            .symlink_metadata()
            .ok()
            .map(|metadata| integrity::Stamp::new(&metadata));
        let limit = {
            let mut pool = self.pool.lock().expect("Python watcher pool lock is valid");
            pool.remove(&self.state);
            let mut state = self.state.lock().expect("Python watcher lock is valid");
            state.clear();
            if state
                .rejected
                .as_ref()
                .is_some_and(|(attempted, previous)| {
                    attempted.elapsed() < INTERVAL && previous == &stamp
                })
            {
                return;
            }
            state.rejected = Some((Instant::now(), stamp));
            pool.directories
        };
        // Directory scans never hold the shared admission lock; ready entries
        // in other environments only contend with bounded native registration.
        let Some(coverage) = coverage(root, limit) else {
            return;
        };
        let mut pool = self.pool.lock().expect("Python watcher pool lock is valid");
        pool.remove(&self.state);
        pool.reserve(coverage.len());
        let Ok(backend) =
            RecommendedWatcher::new(handler, Config::default().with_follow_symlinks(false))
        else {
            return;
        };
        let mut state = self.state.lock().expect("Python watcher lock is valid");
        state.backend = Some(backend);
        state.coverage = coverage;
        let State {
            backend, coverage, ..
        } = &mut *state;
        let registered = coverage.iter().all(|(path, _)| {
            backend
                .as_mut()
                .expect("Python watcher is configured")
                .watch(path, RecursiveMode::NonRecursive)
                .is_ok()
        });
        if !registered {
            state.clear();
            return;
        }
        state.rejected = None;
        pool.entries.push_back(Arc::downgrade(&self.state));
    }
}

fn coverage(root: &Path, limit: usize) -> Option<Coverage> {
    let mut pending = vec![root.to_owned()];
    let mut result = Vec::new();
    while let Some(path) = pending.pop() {
        let metadata = path.symlink_metadata().ok()?;
        if !metadata.is_dir() || metadata.is_symlink() || result.len() >= limit {
            return None;
        }
        result.push((path.clone(), integrity::Stamp::new(&metadata)));
        for entry in std::fs::read_dir(path).ok()? {
            let entry = entry.ok()?;
            let metadata = entry.path().symlink_metadata().ok()?;
            if metadata.is_dir() && !metadata.is_symlink() {
                if result.len() + pending.len() >= limit {
                    return None;
                }
                pending.push(entry.path());
            }
        }
    }
    result.sort_by(|(left, _), (right, _)| left.cmp(right));
    Some(result)
}

#[cfg(test)]
mod tests;
