use super::*;
#[test]
fn increments_versions_and_evicts_the_oldest() {
    let mut documents = Documents::default();
    let text = || Text {
        text: "first".into(),
        revision: "a".into(),
    };
    assert_eq!(
        documents.put("first.rs".into(), text(), "saved".into()),
        (true, true, 1, None)
    );
    assert_eq!(
        documents.put("first.rs".into(), text(), "saved".into()),
        (false, false, 1, None)
    );
    assert_eq!(
        documents.put(
            "first.rs".into(),
            Text {
                text: "second".into(),
                revision: "b".into()
            },
            "saved".into()
        ),
        (false, true, 2, None)
    );
    for index in 1..MAX_DOCUMENTS {
        documents.put(format!("{index}.rs"), text(), "saved".into());
    }
    assert_eq!(
        documents
            .put("last.rs".into(), text(), "saved".into())
            .3
            .as_deref(),
        Some("first.rs")
    );
    assert_eq!(documents.0.len(), MAX_DOCUMENTS);
}
#[test]
fn confines_symlinks_and_encoded_uris() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("中文 %.rs"), "data").unwrap();
    let address = uri(&root, "中文 %.rs").unwrap();
    assert_eq!(relative(&root, &address).as_deref(), Some("中文 %.rs"));
    std::fs::write(directory.path().join("outside.rs"), "secret").unwrap();
    assert!(
        relative(
            &root,
            url::Url::from_file_path(directory.path().join("outside.rs"))
                .unwrap()
                .as_str()
        )
        .is_none()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(directory.path().join("outside.rs"), root.join("escape.rs"))
            .unwrap();
        assert!(read(&root, "escape.rs").is_err());
        assert!(relative(&root, &self::uri(&root, "escape.rs").unwrap()).is_none());
    }
    assert!(uri(&root, "../outside.rs").is_err());
}
#[test]
fn converts_windows_file_uri() {
    let uri = url::Url::parse("file:///C:/project/hello%20world.rs").unwrap();
    assert_eq!(uri.scheme(), "file");
    assert_eq!(uri.path(), "/C:/project/hello%20world.rs");
    #[cfg(windows)]
    assert_eq!(
        uri.to_file_path().unwrap(),
        PathBuf::from("C:\\project\\hello world.rs")
    );
}
