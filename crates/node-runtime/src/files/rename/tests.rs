use super::*;

mod between;

#[test]
fn preserves_contents_on_conflict() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("source")).unwrap();
    std::fs::create_dir(root.join("资料")).unwrap();
    std::fs::write(root.join("source/file"), "content").unwrap();
    rename(&root, "source/file", "资料/文件").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("资料/文件")).unwrap(),
        "content"
    );
    rename(&root, "资料", "renamed").unwrap();
    rename(&root, "renamed", "renamed").unwrap();
    assert_eq!(
        rename(&root, "renamed/文件", "source").unwrap_err().code,
        ErrorCode::Conflict
    );
    std::fs::write(root.join("existing"), "keep").unwrap();
    assert_eq!(
        rename(&root, "renamed/文件", "existing").unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        std::fs::read_to_string(root.join("existing")).unwrap(),
        "keep"
    );
    assert!(root.join("renamed/文件").exists());
}

#[test]
fn rejects_invalid_destinations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("file"), "keep").unwrap();
    for path in ["", "../outside", "/absolute", "dir/../file"] {
        assert_eq!(
            rename(&root, "file", path).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            rename(&root, path, "other").unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    for path in [".git", ".GIT/config", "nested/.git"] {
        assert_eq!(
            rename(&root, "file", path).unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(
            rename(&root, path, "other").unwrap_err().code,
            ErrorCode::PermissionDenied
        );
    }
    assert_eq!(
        rename(&root, "file", "missing/file").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        rename(&root, "missing", "file").unwrap_err().code,
        ErrorCode::NotFound
    );
    assert!(root.join("file").exists());
}

#[cfg(unix)]
#[test]
fn preserves_links() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("file"), "keep").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("linked")).unwrap();
    assert!(rename(&root, "linked/file", "escaped").is_err());
    std::fs::write(root.join("file"), "inside").unwrap();
    assert!(rename(&root, "file", "linked/escaped").is_err());
    assert_eq!(
        rename(&root, "file", "linked").unwrap_err().code,
        ErrorCode::Conflict
    );
    rename(&root, "linked", "moved-link").unwrap();
    assert!(
        root.join("moved-link")
            .symlink_metadata()
            .unwrap()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("file")).unwrap(),
        "keep"
    );
    assert!(!outside.path().join("escaped").exists());
    std::os::unix::fs::symlink("missing", root.join("broken")).unwrap();
    assert_eq!(
        rename(&root, "file", "broken").unwrap_err().code,
        ErrorCode::Conflict
    );
    rename(&root, "broken", "moved-broken").unwrap();
    assert_eq!(
        std::fs::read_link(root.join("moved-broken")).unwrap(),
        Path::new("missing")
    );
}
