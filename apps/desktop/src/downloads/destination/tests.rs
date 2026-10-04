use super::*;
use std::io::Write;

fn staged(path: PathBuf) -> Destination {
    let destination = Destination::prepare(path).unwrap();
    destination
        .writer()
        .unwrap()
        .write_all(b"downloaded")
        .unwrap();
    destination
}

#[test]
fn publishes_selected_targets() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("资料.bin");
    staged(path.clone())
        .publish(&CancellationToken::new())
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"downloaded");
    fs::write(&path, "original").unwrap();
    staged(path.clone())
        .publish(&CancellationToken::new())
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"downloaded");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn preserves_concurrent_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("target");
    for existing in [false, true] {
        if existing {
            fs::write(&path, "old").unwrap();
        }
        let destination = staged(path.clone());
        fs::write(&path, "external change").unwrap();
        assert_eq!(
            destination.publish(&CancellationToken::new()),
            Err("files_download_conflict")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
    }
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn cleans_owned_staging() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("target");
    fs::write(&path, "original").unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        staged(path.clone()).publish(&cancel),
        Err("files_download_cancelled")
    );
    drop(staged(path.clone()));
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn rejects_invalid_targets() {
    let temp = tempfile::tempdir().unwrap();
    assert!(Destination::prepare(PathBuf::from("relative")).is_err());
    assert!(Destination::prepare(temp.path().to_path_buf()).is_err());
    assert!(Destination::prepare(temp.path().join("missing/target")).is_err());
    let path = temp.path().join("target");
    fs::write(&path, "original").unwrap();
    let destination = staged(path.clone());
    fs::remove_file(&path).unwrap();
    assert_eq!(
        destination.publish(&CancellationToken::new()),
        Err("files_download_conflict")
    );
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn rejects_retargeted_paths() {
    let temp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let path = temp.path().join("target");
    fs::write(other.path().join("target"), "outside").unwrap();
    std::os::unix::fs::symlink(other.path().join("target"), &path).unwrap();
    assert!(Destination::prepare(path).is_err());
    let parent = temp.path().join("parent");
    let original = temp.path().join("original");
    fs::create_dir(&original).unwrap();
    std::os::unix::fs::symlink(&original, &parent).unwrap();
    let destination = staged(parent.join("target"));
    fs::remove_file(&parent).unwrap();
    std::os::unix::fs::symlink(other.path(), &parent).unwrap();
    assert_eq!(
        destination.publish(&CancellationToken::new()),
        Err("files_download_conflict")
    );
    assert_eq!(
        fs::read_to_string(other.path().join("target")).unwrap(),
        "outside"
    );
    assert_eq!(fs::read_dir(original).unwrap().count(), 0);
}
