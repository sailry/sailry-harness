use super::*;

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[test]
fn chinese_paths_and_content() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir(root.join("资料")).unwrap();
    let target = root.join("资料/中文 文件.txt");
    let created = save(&root, "资料/中文 文件.txt", "初始内容", None).unwrap();
    let updated = save(
        &root,
        "资料/中文 文件.txt",
        "更新内容",
        Some(&created.revision),
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "更新内容");
    assert_eq!(updated.revision, revision("更新内容"));
    assert_eq!(std::fs::read_dir(root.join("资料")).unwrap().count(), 1);
}

#[test]
fn conflict_preserves_content() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("file"), "original").unwrap();
    let fault = publish(
        &root,
        "file",
        "ours",
        Some(&revision("original")),
        &|phase| {
            if matches!(phase, Phase::Prepared) {
                std::fs::write(root.join("file"), "external").unwrap();
            }
        },
    )
    .unwrap_err();
    assert_eq!(fault.code, ErrorCode::RevisionConflict);
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "external"
    );
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
}

#[test]
fn creates_exclusively() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let fault = publish(&root, "file", "ours", None, &|phase| {
        if matches!(phase, Phase::Prepared) {
            std::fs::write(root.join("file"), "external").unwrap();
        }
    })
    .unwrap_err();
    assert_eq!(fault.code, ErrorCode::RevisionConflict);
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "external"
    );
}

#[test]
fn preserves_later_external_write() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("file"), "original").unwrap();
    let fault = publish(
        &root,
        "file",
        "ours",
        Some(&revision("original")),
        &|phase| {
            if matches!(phase, Phase::Published) {
                std::fs::write(root.join("file"), "external").unwrap();
            }
        },
    )
    .unwrap_err();
    assert_eq!(fault.code, ErrorCode::OutcomeUnknown);
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "external"
    );
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
}

#[test]
fn rejects_invalid_input() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for (path, text, expected) in [
        ("../outside", "text", None),
        ("file", "binary\0text", None),
        ("file", "text", Some("invalid")),
    ] {
        assert!(save(&root, path, text, expected).is_err());
    }
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn preserves_readonly_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("file");
    std::fs::write(&path, "original").unwrap();
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).unwrap();
    let result = save(&root, "file", "ours", Some(&revision("original")));
    let retained = std::fs::metadata(&path).unwrap().permissions().readonly();
    std::fs::set_permissions(&path, original).unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::PermissionDenied);
    assert!(retained);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn preserves_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let target = root.join("file");
        std::fs::write(&target, "original").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        save(&root, "file", "ours", Some(&revision("original"))).unwrap();
        assert_eq!(
            std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn rejects_symlink_redirect() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("external"), "external").unwrap();
        symlink(root.join("external"), root.join("file")).unwrap();
        assert_eq!(
            save(&root, "file", "ours", Some(&revision("external")))
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            std::fs::read_to_string(root.join("external")).unwrap(),
            "external"
        );
        assert!(root.join("file").symlink_metadata().unwrap().is_symlink());
    }

    #[test]
    fn detects_moved_parent() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("file"), "original").unwrap();
        let result = publish(
            &root,
            "file",
            "ours",
            Some(&revision("original")),
            &|phase| {
                if matches!(phase, Phase::Prepared) {
                    std::fs::rename(&root, base.join("moved")).unwrap();
                    std::fs::create_dir(&root).unwrap();
                }
            },
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::RevisionConflict);
        assert_eq!(
            std::fs::read_to_string(base.join("moved/file")).unwrap(),
            "original"
        );
        assert_eq!(std::fs::read_dir(base.join("moved")).unwrap().count(), 1);
        assert_eq!(std::fs::read_dir(root).unwrap().count(), 0);
    }
}

#[test]
fn creates_missing_parents_for_text_and_artifacts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    save(&root, "output/资料/report.txt", "Report", None).unwrap();
    save_bytes(
        &root,
        "images/nested/image.png",
        b"binary fixture",
        None,
        1024,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("output/资料/report.txt")).unwrap(),
        "Report"
    );
    assert_eq!(
        std::fs::read(root.join("images/nested/image.png")).unwrap(),
        b"binary fixture"
    );
    assert!(
        save(
            &root,
            "absent/report.txt",
            "replace",
            Some(&revision("old"))
        )
        .is_err()
    );
    assert!(!root.join("absent").exists());
    assert!(save(&root, "output/资料/report.txt/nested/file", "bad", None).is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("output/资料/report.txt")).unwrap(),
        "Report"
    );
}

#[cfg(unix)]
#[test]
fn creation_rejects_linked_parents() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("linked")).unwrap();
    assert!(save(&root, "linked/missing/file.txt", "bad", None).is_err());
    assert!(!outside.path().join("missing").exists());
    assert!(save(&root, "new/../bad/file.txt", "bad", None).is_err());
    assert!(!root.join("new").exists());
}
