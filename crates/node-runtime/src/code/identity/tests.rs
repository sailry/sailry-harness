use super::*;
use std::fs::FileTimes;

#[test]
fn reuses_unchanged_bytes_and_refreshes_modified_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server");
    std::fs::write(&path, "first").unwrap();
    let cache = Cache::default();
    let original = cache.revision(&path).unwrap();
    let modified = path.metadata().unwrap().modified().unwrap();
    assert_eq!(cache.revision(&path).unwrap(), original);
    assert_eq!(cache.reads(), 1);
    std::fs::write(&path, "other").unwrap();
    File::open(&path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified + std::time::Duration::from_secs(1)))
        .unwrap();
    let replacement = cache.revision(&path).unwrap();
    assert_ne!(replacement, original);
    assert_eq!(cache.reads(), 2);
    assert_eq!(cache.revision(&path).unwrap(), replacement);
    assert_eq!(cache.reads(), 2);
    std::fs::remove_file(&path).unwrap();
    assert!(cache.revision(&path).is_err());
}

#[cfg(unix)]
#[test]
fn invalidates_replacements_preserving_size_and_timestamp() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server");
    let replacement = directory.path().join("replacement");
    std::fs::write(&path, "first").unwrap();
    let modified = path.metadata().unwrap().modified().unwrap();
    let cache = Cache::default();
    let original = cache.revision(&path).unwrap();
    std::fs::write(&replacement, "other").unwrap();
    File::open(&replacement)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    std::fs::rename(&replacement, &path).unwrap();
    assert_eq!(path.metadata().unwrap().modified().unwrap(), modified);
    assert_ne!(cache.revision(&path).unwrap(), original);
    assert_eq!(cache.reads(), 2);
}

#[cfg(unix)]
#[test]
fn shares_canonical_paths_and_tracks_symlink_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server");
    let replacement = directory.path().join("replacement");
    let alias = directory.path().join("alias");
    std::fs::write(&path, "first").unwrap();
    std::fs::write(&replacement, "other").unwrap();
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let cache = Cache::default();
    let original = cache.revision(&path).unwrap();
    assert_eq!(cache.revision(&alias).unwrap(), original);
    assert_eq!(cache.reads(), 1);
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&replacement, &alias).unwrap();
    assert_ne!(cache.revision(&alias).unwrap(), original);
    assert_eq!(cache.reads(), 2);
}

#[test]
fn shares_the_initial_read_between_concurrent_queries() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server");
    std::fs::write(&path, [1; 64 * 1024]).unwrap();
    let cache = std::sync::Arc::new(Cache::default());
    let tasks = (0..8)
        .map(|_| {
            let cache = cache.clone();
            let path = path.clone();
            std::thread::spawn(move || cache.revision(&path).unwrap())
        })
        .collect::<Vec<_>>();
    let revisions = tasks
        .into_iter()
        .map(|task| task.join().unwrap())
        .collect::<Vec<_>>();
    assert!(revisions.iter().all(|revision| revision == &revisions[0]));
    assert_eq!(cache.reads(), 1);
}

#[test]
fn rejects_oversized_files_before_reading() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("server");
    File::create(&path).unwrap().set_len(MAX_BYTES + 1).unwrap();
    let cache = Cache::default();
    assert_eq!(
        cache.revision(&path).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(cache.reads(), 0);
}
