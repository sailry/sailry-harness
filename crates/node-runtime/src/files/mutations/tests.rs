use super::*;
use sailry_protocol::ErrorCode;

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[test]
fn move_checks_source_and_destination() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::write(root.join("source"), "current").unwrap();
    std::fs::write(root.join("existing"), "keep").unwrap();
    for (to, expected, code) in [
        ("new", revision("stale"), ErrorCode::RevisionConflict),
        ("existing", revision("current"), ErrorCode::Conflict),
        ("new", "invalid".into(), ErrorCode::InvalidRequest),
    ] {
        assert_eq!(
            move_file(&root, "source", to, &expected).unwrap_err().code,
            code
        );
        assert_eq!(
            std::fs::read_to_string(root.join("source")).unwrap(),
            "current"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("existing")).unwrap(),
            "keep"
        );
        assert!(!root.join("new").exists());
    }
    move_file(&root, "source", "new", &revision("current")).unwrap();
    assert!(!root.join("source").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("new")).unwrap(),
        "current"
    );
}

#[test]
fn delete_rejects_changes_and_directories() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    std::fs::write(root.join("source"), "current").unwrap();
    std::fs::create_dir(root.join("folder")).unwrap();
    assert_eq!(
        delete(&root, "source", &revision("stale"))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        delete(&root, "folder", &revision("")).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        std::fs::read_to_string(root.join("source")).unwrap(),
        "current"
    );
    assert!(root.join("folder").is_dir());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_targets_and_parents() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("source"), "private").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("linked")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("source"), root.join("source")).unwrap();
    for path in ["linked/source", "source", "../source"] {
        assert!(move_file(&root, path, "new", &revision("private")).is_err());
        assert!(delete(&root, path, &revision("private")).is_err());
    }
    assert_eq!(
        std::fs::read_to_string(outside.path().join("source")).unwrap(),
        "private"
    );
}
