use super::*;

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[test]
fn preserves_large_files_and_crlf() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let prefix = "unchanged 🙂\r\n".repeat(20_000);
    let before = format!("{prefix}old target\r\nlast\r\n");
    std::fs::write(root.path().join("file"), &before).unwrap();
    let written = edit(
        &path,
        "file",
        "old target",
        "new target",
        &revision(&before),
    )
    .unwrap();
    let expected = format!("{prefix}new target\r\nlast\r\n");
    assert_eq!(
        std::fs::read(root.path().join("file")).unwrap(),
        expected.as_bytes()
    );
    assert_eq!(written.revision, revision(&expected));
    assert_eq!(written.size, expected.len() as u64);
}

#[test]
fn rejects_missing_and_ambiguous_matches() {
    for (before, old) in [
        ("first first", "first"),
        ("aaa", "aa"),
        ("🙂🙂🙂", "🙂🙂"),
        ("target", "missing"),
        ("target", ""),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file"), before).unwrap();
        assert_eq!(
            edit(
                &root.path().canonicalize().unwrap(),
                "file",
                old,
                "new",
                &revision(before)
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("file")).unwrap(),
            before
        );
    }
}

#[test]
fn rejects_stale_revisions_and_invalid_content() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), "current").unwrap();
    assert_eq!(
        edit(
            &root.path().canonicalize().unwrap(),
            "file",
            "current",
            "changed",
            &revision("old")
        )
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
    for replacement in ["\0".into(), "x".repeat(MAX_FILE_BYTES + 1)] {
        assert_eq!(
            edit(
                &root.path().canonicalize().unwrap(),
                "file",
                "current",
                &replacement,
                &revision("current")
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("file")).unwrap(),
        "current"
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlink_escape() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("file"), "original").unwrap();
    std::os::unix::fs::symlink(outside.path().join("file"), root.path().join("file")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("parent")).unwrap();
    let path = root.path().canonicalize().unwrap();
    for relative in ["file", "parent/file", "../file"] {
        assert!(
            edit(
                &path,
                relative,
                "original",
                "changed",
                &revision("original")
            )
            .is_err()
        );
    }
    assert_eq!(
        std::fs::read_to_string(outside.path().join("file")).unwrap(),
        "original"
    );
}
