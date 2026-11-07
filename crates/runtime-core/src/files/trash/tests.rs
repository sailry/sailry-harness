use super::*;

mod checked;

#[test]
fn validates_before_native_call() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for path in [
        "",
        "../outside",
        "/absolute",
        "folder/../file",
        ".git",
        "a/.GIT/config",
        "missing",
    ] {
        assert!(
            relocate(&root, path, |_| panic!(
                "invalid path reached native adapter"
            ))
            .is_err()
        );
    }
    std::fs::create_dir(root.join("folder")).unwrap();
    std::fs::write(root.join("folder/文件"), "preserved").unwrap();
    let recovered = tempfile::tempdir().unwrap();
    relocate(&root, "folder", |path| {
        std::fs::rename(path, recovered.path().join("folder")).map_err(io_error)
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(recovered.path().join("folder/文件")).unwrap(),
        "preserved"
    );
}

#[test]
fn preserves_uncertain_outcome() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join("file"), "preserved").unwrap();
    assert_eq!(
        relocate(&root, "file", |_| Err(unknown()))
            .unwrap_err()
            .code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        relocate(&root, "file", |_| Ok(())).unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        std::fs::read_to_string(root.join("file")).unwrap(),
        "preserved"
    );
}

#[cfg(unix)]
#[test]
fn rechecks_path_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("file"), "preserved").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.join("linked")).unwrap();
    assert!(relocate(&root, "linked/file", |_| panic!("followed symlink parent")).is_err());
    relocate(&root, "linked", |path| {
        assert!(path.symlink_metadata().unwrap().is_symlink());
        std::fs::rename(path, root.join("recovered-link")).map_err(io_error)
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(outside.path().join("file")).unwrap(),
        "preserved"
    );
    std::fs::create_dir(root.join("folder")).unwrap();
    std::fs::write(root.join("folder/file"), "preserved").unwrap();
    assert_eq!(
        relocate(&root, "folder/file", |path| {
            std::fs::rename(path, root.join("recovered")).unwrap();
            std::fs::rename(root.join("folder"), root.join("moved")).unwrap();
            std::fs::create_dir(root.join("folder")).unwrap();
            Ok(())
        })
        .unwrap_err()
        .code,
        ErrorCode::OutcomeUnknown
    );
    assert!(root.join("recovered").exists());
}
