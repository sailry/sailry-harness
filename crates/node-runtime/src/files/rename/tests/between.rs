use super::*;

#[test]
fn moves_matching_names() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("source");
    let target = root.join("target");
    std::fs::create_dir_all(source.join("资料/sub/empty")).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(source.join("资料/sub/file"), "saved content").unwrap();
    let metadata = std::fs::metadata(source.join("资料/sub/file")).unwrap();
    between(&source, &target, "资料", "资料").unwrap();
    assert!(!source.join("资料").exists());
    assert!(target.join("资料/sub/empty").is_dir());
    assert_eq!(
        std::fs::read_to_string(target.join("资料/sub/file")).unwrap(),
        "saved content"
    );
    let moved = std::fs::metadata(target.join("资料/sub/file")).unwrap();
    assert_eq!(moved.permissions(), metadata.permissions());
    assert_eq!(moved.modified().unwrap(), metadata.modified().unwrap());
    between(&target, &source, "资料/sub/file", "file").unwrap();
    assert_eq!(
        std::fs::read_to_string(source.join("file")).unwrap(),
        "saved content"
    );
    assert!(!target.join("资料/sub/file").exists());
    std::fs::write(target.join("file"), "preserved").unwrap();
    assert_eq!(
        between(&source, &target, "file", "file").unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        std::fs::read_to_string(target.join("file")).unwrap(),
        "preserved"
    );
    assert!(source.join("file").exists());
}

#[test]
fn rejects_descendant_targets() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("source/child")).unwrap();
    std::fs::write(root.join("source/file"), "preserved").unwrap();
    assert!(between(&root, &root.join("source/child"), "source", "moved").is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("source/file")).unwrap(),
        "preserved"
    );
    assert!(!root.join("source/child/moved").exists());
}

#[cfg(unix)]
#[test]
fn rechecks_parent_identity() {
    for changed_source in [false, true] {
        for moved in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let source = root.join("source");
            let target = root.join("target");
            std::fs::create_dir(&source).unwrap();
            std::fs::create_dir(&target).unwrap();
            std::fs::write(source.join("file"), "preserved").unwrap();
            let displaced = temp.path().join("displaced");
            let error = relocate(&source, &target, "file", "file", &|phase| {
                if matches!(phase, Phase::Moved) == moved {
                    let parent = if changed_source { &source } else { &target };
                    std::fs::rename(parent, &displaced).unwrap();
                    std::fs::create_dir(parent).unwrap();
                    std::fs::write(parent.join("sentinel"), "replacement").unwrap();
                }
            })
            .unwrap_err();
            assert_eq!(
                error.code,
                if moved {
                    ErrorCode::OutcomeUnknown
                } else {
                    ErrorCode::RevisionConflict
                }
            );
            let content = if moved {
                if changed_source {
                    target.join("file")
                } else {
                    displaced.join("file")
                }
            } else if changed_source {
                displaced.join("file")
            } else {
                source.join("file")
            };
            assert_eq!(std::fs::read_to_string(content).unwrap(), "preserved");
            let replacement = if changed_source { &source } else { &target };
            assert_eq!(
                std::fs::read_to_string(replacement.join("sentinel")).unwrap(),
                "replacement"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn confines_link_moves() {
    let source = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let source_root = source.path().canonicalize().unwrap();
    let target_root = target.path().canonicalize().unwrap();
    std::fs::write(outside.path().join("file"), "preserved").unwrap();
    std::os::unix::fs::symlink(outside.path(), source.path().join("link")).unwrap();
    std::os::unix::fs::symlink(outside.path(), target.path().join("link")).unwrap();
    assert!(between(&source_root, &target_root, "link/file", "file").is_err());
    std::fs::write(source.path().join("file"), "saved").unwrap();
    assert!(between(&source_root, &target_root, "file", "link/file").is_err());
    between(&source_root, &target_root, "link", "moved").unwrap();
    assert!(
        target
            .path()
            .join("moved")
            .symlink_metadata()
            .unwrap()
            .is_symlink()
    );
    assert!(source.path().join("file").exists());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("file")).unwrap(),
        "preserved"
    );
}
