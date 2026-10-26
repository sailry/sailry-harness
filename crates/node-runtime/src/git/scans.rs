//! Full worktree inspections share one permit for each registered root.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::Semaphore;

#[derive(Default)]
pub(super) struct Scans(Mutex<BTreeMap<PathBuf, Weak<Semaphore>>>);

impl Scans {
    pub(super) fn root(&self, root: &Path) -> Arc<Semaphore> {
        let mut roots = self.0.lock().unwrap_or_else(|error| error.into_inner());
        roots.retain(|_, gate| gate.strong_count() > 0);
        if let Some(gate) = roots.get(root).and_then(Weak::upgrade) {
            return gate;
        }
        let gate = Arc::new(Semaphore::new(1));
        roots.insert(root.to_owned(), Arc::downgrade(&gate));
        gate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serializes_only_the_same_root() {
        let scans = Scans::default();
        let first = scans.root(Path::new("first"));
        let permit = first.clone().acquire_owned().await.unwrap();
        let same = scans.root(Path::new("first"));
        assert!(Arc::ptr_eq(&first, &same));
        assert!(same.try_acquire().is_err());
        assert!(scans.root(Path::new("second")).try_acquire().is_ok());
        drop(permit);
        assert!(same.try_acquire().is_ok());
    }
}
