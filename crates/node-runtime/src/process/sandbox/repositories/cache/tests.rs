use super::*;
use std::{
    fs::{self, Permissions},
    os::unix::{ffi::OsStringExt, fs::PermissionsExt},
    sync::atomic::Ordering,
};

fn metadata(path: &Path) {
    fs::create_dir_all(path.join("objects")).unwrap();
    fs::write(path.join("HEAD"), "ref: refs/heads/main\n").unwrap();
}

#[test]
fn reuses_directory_witnesses_and_discovers_new_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let nested = root.join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("source"), "first").unwrap();
    let cache = Cache::default();
    assert!(cache.discover(&root).unwrap().is_empty());
    assert!(cache.discover(&root).unwrap().is_empty());
    assert_eq!(cache.scans.load(Ordering::Relaxed), 1);
    fs::write(nested.join("source"), "changed source contents").unwrap();
    assert!(cache.discover(&root).unwrap().is_empty());
    assert_eq!(cache.scans.load(Ordering::Relaxed), 1);
    metadata(&nested.join(".git"));
    assert!(
        cache
            .discover(&root)
            .unwrap()
            .contains(&nested.join(".git"))
    );
    assert_eq!(cache.scans.load(Ordering::Relaxed), 2);
    assert!(
        cache
            .discover(&root)
            .unwrap()
            .contains(&nested.join(".git"))
    );
    assert_eq!(cache.scans.load(Ordering::Relaxed), 2);
    fs::remove_dir_all(nested.join(".git")).unwrap();
    assert!(cache.discover(&root).unwrap().is_empty());
    assert_eq!(cache.scans.load(Ordering::Relaxed), 3);
}

#[test]
fn refreshes_pointers_commondirs_and_ignore_rules() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    let root = base.join("work");
    let first = base.join("first");
    let other = base.join("other");
    let linked = base.join("linked");
    fs::create_dir(&root).unwrap();
    metadata(&first);
    metadata(&other);
    fs::create_dir(&linked).unwrap();
    fs::write(linked.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(linked.join("commondir"), "../first\n").unwrap();
    fs::write(root.join(".git"), "gitdir: ../linked\n").unwrap();
    let nested = root.join("nested");
    metadata(&nested.join(".git"));
    fs::write(root.join(".gitignore"), "nested/\n").unwrap();
    let cache = Cache::default();
    let initial = cache.discover(&root).unwrap();
    assert!(initial.contains(&linked));
    assert!(initial.contains(&first));
    assert!(!initial.contains(&other));
    assert!(!initial.contains(&nested.join(".git")));
    fs::write(linked.join("commondir"), "../other\n").unwrap();
    let changed = cache.discover(&root).unwrap();
    assert!(changed.contains(&other));
    assert!(!changed.contains(&first));
    fs::write(root.join(".gitignore"), "").unwrap();
    assert!(
        cache
            .discover(&root)
            .unwrap()
            .contains(&nested.join(".git"))
    );
    fs::write(root.join(".git"), "gitdir: ../first\n").unwrap();
    let moved = cache.discover(&root).unwrap();
    assert!(moved.contains(&first));
    assert!(!moved.contains(&linked));
    assert!(!moved.contains(&other));
    assert_eq!(cache.scans.load(Ordering::Relaxed), 4);
}

#[test]
fn tracks_git_targets_without_traversing_workspace_links() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    let root = base.join("work");
    fs::create_dir(&root).unwrap();
    let first = base.join("first");
    let other = base.join("other");
    metadata(&first);
    metadata(&other);
    let alias = base.join("alias");
    std::os::unix::fs::symlink(&first, &alias).unwrap();
    fs::write(root.join(".git"), "gitdir: ../alias\n").unwrap();
    std::os::unix::fs::symlink(&other, root.join("workspace-alias")).unwrap();
    let cache = Cache::default();
    let initial = cache.discover(&root).unwrap();
    assert!(initial.contains(&first));
    assert!(!initial.contains(&other));
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&other, &alias).unwrap();
    let moved = cache.discover(&root).unwrap();
    assert!(moved.contains(&other));
    assert!(!moved.contains(&first));
    assert_eq!(cache.scans.load(Ordering::Relaxed), 2);
}

#[test]
fn retains_native_names_and_unreadable_directory_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let native = root.join(if cfg!(target_os = "linux") {
        std::ffi::OsString::from_vec(b"native-\xff".to_vec())
    } else {
        std::ffi::OsString::from("native-资料")
    });
    metadata(&native.join(".git"));
    let restricted = root.join("restricted");
    metadata(&restricted.join(".git"));
    fs::set_permissions(&restricted, Permissions::from_mode(0)).unwrap();
    let inaccessible = crate::files::path::root(&restricted).is_err();
    let cache = Cache::default();
    let initial = cache.discover(&root).unwrap();
    assert!(initial.contains(&native.join(".git")));
    if inaccessible {
        assert!(!initial.contains(&restricted.join(".git")));
    }
    assert_eq!(cache.discover(&root).unwrap(), initial);
    assert_eq!(cache.scans.load(Ordering::Relaxed), 1);
    fs::set_permissions(&restricted, Permissions::from_mode(0o700)).unwrap();
    assert!(
        cache
            .discover(&root)
            .unwrap()
            .contains(&restricted.join(".git"))
    );
    assert_eq!(cache.scans.load(Ordering::Relaxed), 2);
}

#[cfg(target_os = "linux")]
#[test]
fn accepts_native_git_pointer_targets() {
    let directory = tempfile::tempdir().unwrap();
    let base = directory.path().canonicalize().unwrap();
    let root = base.join("work");
    fs::create_dir(&root).unwrap();
    let native = base.join(std::ffi::OsString::from_vec(b"metadata-\xff".to_vec()));
    metadata(&native);
    fs::write(root.join(".git"), b"gitdir: ../metadata-\xff\n").unwrap();
    assert!(Cache::default().discover(&root).unwrap().contains(&native));
}
