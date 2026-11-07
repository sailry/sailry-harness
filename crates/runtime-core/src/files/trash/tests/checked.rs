use super::*;
use crate::files::version;

fn revision(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[test]
fn verifies_source_identity() {
    for bytes in [vec![], vec![0xff; 10 * 1024 * 1024]] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("file"), &bytes).unwrap();
        let stamp = version::current(&root, "file").unwrap();
        checked(&root, "file", &revision(&bytes), &stamp, |path| {
            std::fs::rename(path, root.join("recovered")).map_err(io_error)
        })
        .unwrap();
        assert_eq!(std::fs::read(root.join("recovered")).unwrap(), bytes);
        assert!(!root.join("file").exists());
    }
}

#[test]
fn rejects_changed_sources() {
    for change in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("folder")).unwrap();
        let file = root.join("folder/file");
        std::fs::write(&file, "original").unwrap();
        let original_permissions = std::fs::metadata(&file).unwrap().permissions();
        let stamp = version::current(&root, "folder/file").unwrap();
        match change {
            0 => std::fs::write(&file, "modified").unwrap(),
            1 => {
                std::fs::rename(&file, root.join("retained")).unwrap();
                std::fs::write(&file, "original").unwrap();
            }
            2 => {
                std::fs::rename(root.join("folder"), root.join("retained")).unwrap();
                std::fs::create_dir(root.join("folder")).unwrap();
                std::fs::rename(root.join("retained/file"), &file).unwrap();
            }
            3 => {
                let mut permissions = std::fs::metadata(&file).unwrap().permissions();
                permissions.set_readonly(true);
                std::fs::set_permissions(&file, permissions).unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            checked(&root, "folder/file", &revision(b"original"), &stamp, |_| {
                panic!("changed source reached Trash")
            })
            .unwrap_err()
            .code,
            ErrorCode::RevisionConflict
        );
        assert!(file.exists());
        std::fs::set_permissions(file, original_permissions).unwrap();
    }
}

#[test]
fn checks_unstamped_content_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("file"), "contents").unwrap();
    let stamp = version::current(&root, "file").unwrap();
    assert_eq!(
        checked(&root, "file", &revision(b"different"), &stamp, |_| {
            panic!("unverified content reached Trash")
        })
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
    for (revision, stamp) in [("invalid", stamp.as_str()), (stamp.as_str(), "invalid")] {
        assert_eq!(
            checked(&root, "file", revision, stamp, |_| {
                panic!("invalid revision reached Trash")
            })
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(
        checked(&root, "file", &revision(b"contents"), &stamp, |_| Err(
            unknown()
        ))
        .unwrap_err()
        .code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"contents");
}

#[cfg(unix)]
#[test]
fn rejects_changed_entry_types() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("file"), "original").unwrap();
    let stamp = version::current(&root, "file").unwrap();
    std::fs::rename(root.join("file"), root.join("retained")).unwrap();
    std::os::unix::fs::symlink(root.join("retained"), root.join("file")).unwrap();
    assert!(
        checked(&root, "file", &revision(b"original"), &stamp, |_| {
            panic!("symbolic link reached checked Trash")
        })
        .is_err()
    );
    std::fs::create_dir(root.join("folder")).unwrap();
    assert!(
        checked(&root, "folder", &revision(b"original"), &stamp, |_| {
            panic!("directory reached checked Trash")
        })
        .is_err()
    );
    assert_eq!(std::fs::read(root.join("retained")).unwrap(), b"original");
}
