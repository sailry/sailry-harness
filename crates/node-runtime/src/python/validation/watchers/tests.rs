use super::*;

fn fixture(children: usize) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for index in 0..children {
        std::fs::create_dir(directory.path().join(index.to_string())).unwrap();
    }
    directory
}

fn attach(pool: &Arc<Mutex<Pool>>, root: &Path) -> Watch {
    let watch = Watch::with_pool(pool.clone());
    watch.watch(root, |_| {});
    assert!(watch.active());
    watch
}

#[test]
fn bounds_directory_subscriptions() {
    let pool = Arc::new(Mutex::new(Pool::new(4, 8)));
    let first = fixture(2);
    let first = attach(&pool, first.path());
    let second = fixture(1);
    let second = attach(&pool, second.path());
    assert!(!first.active());
    assert!(second.active());
    assert_eq!(second.state.lock().unwrap().coverage.len(), 2);
}

#[test]
fn evicts_least_recently_used() {
    let pool = Arc::new(Mutex::new(Pool::new(16, 2)));
    let first = fixture(0);
    let first_watch = attach(&pool, first.path());
    let second = fixture(0);
    let second_watch = attach(&pool, second.path());
    assert!(first_watch.active());
    let third = fixture(0);
    let third_watch = attach(&pool, third.path());
    assert!(first_watch.active());
    assert!(!second_watch.active());
    assert!(third_watch.active());
}

#[test]
fn releases_dropped_entries() {
    let pool = Arc::new(Mutex::new(Pool::new(1, 1)));
    let root = fixture(0);
    let watch = attach(&pool, root.path());
    let state = Arc::downgrade(&watch.state);
    drop(watch);
    assert!(state.upgrade().is_none());
    let next = attach(&pool, root.path());
    assert!(next.active());
    assert_eq!(pool.lock().unwrap().entries.len(), 1);
}

#[test]
fn rejects_oversized_trees() {
    let root = fixture(2);
    let watch = Watch::isolated(2, 1);
    watch.watch(root.path(), |_| {});
    assert!(!watch.active());
    assert!(watch.state.lock().unwrap().coverage.is_empty());
}

#[test]
fn counts_bytecode_directories() {
    let root = fixture(0);
    std::fs::create_dir_all(root.path().join("__pycache__/nested")).unwrap();
    let watch = Watch::isolated(3, 1);
    watch.watch(root.path(), |_| {});
    assert!(watch.active());
    assert_eq!(watch.state.lock().unwrap().coverage.len(), 3);
    assert!(watch.covers(root.path()));
}

#[test]
fn detects_changed_directory_coverage() {
    let root = fixture(1);
    let watch = Watch::isolated(4, 1);
    watch.watch(root.path(), |_| {});
    assert!(watch.covers(root.path()));
    std::fs::create_dir(root.path().join("0/new")).unwrap();
    assert!(!watch.covers(root.path()));
    watch.watch(root.path(), |_| {});
    assert!(watch.covers(root.path()));
    assert_eq!(watch.state.lock().unwrap().coverage.len(), 3);
}

#[cfg(unix)]
#[test]
fn does_not_follow_directory_links() {
    let root = fixture(0);
    let outside = fixture(4);
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    let watch = Watch::isolated(1, 1);
    watch.watch(root.path(), |_| {});
    assert!(watch.active());
    assert_eq!(watch.state.lock().unwrap().coverage.len(), 1);
}
