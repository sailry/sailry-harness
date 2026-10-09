use super::*;

fn single(
    root: &std::path::Path,
    path: &str,
    old_text: &str,
    new_text: &str,
    expected: &str,
) -> Result<FileWritten, Fault> {
    edit(
        root,
        path,
        &[FileEdit {
            old_text: old_text.into(),
            new_text: new_text.into(),
            replace_all: false,
        }],
        expected,
    )
}

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

fn change(old: &str, new: &str, replace_all: bool) -> FileEdit {
    FileEdit {
        old_text: old.into(),
        new_text: new.into(),
        replace_all,
    }
}

#[test]
fn applies_batches_against_original_content() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let before = "start\r\n🙂 repeat 🙂\r\nend\r\n";
    std::fs::write(path.join("file"), before).unwrap();
    let edits = [
        change("end", "🙂", false),
        change("🙂", "中文", true),
        change("start", "end", false),
    ];
    let written = edit(&path, "file", &edits, &revision(before)).unwrap();
    let after = "end\r\n中文 repeat 中文\r\n🙂\r\n";
    assert_eq!(std::fs::read_to_string(path.join("file")).unwrap(), after);
    assert_eq!(written.revision, revision(after));
    assert_eq!(written.size, after.len() as u64);
}

#[test]
fn failed_batches_preserve_every_byte() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let before = "first aaa 🙂🙂🙂 last";
    std::fs::write(path.join("file"), before).unwrap();
    for edits in [
        vec![
            change("first", "changed", false),
            change("missing", "found", false),
        ],
        vec![
            change("first", "changed", false),
            change("changed", "again", false),
        ],
        vec![
            change("first", "changed", false),
            change("irst", "partial", false),
        ],
        vec![
            change("first", "changed", false),
            change("first", "duplicate", false),
        ],
        vec![
            change("first", "changed", false),
            change("aa", "short", true),
        ],
        vec![change("🙂🙂", "Unicode", true)],
        vec![change("aaa", "empty", false), change("a", "all", true)],
    ] {
        assert_eq!(
            edit(&path, "file", &edits, &revision(before))
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(std::fs::read(path.join("file")).unwrap(), before.as_bytes());
    }
}

#[test]
fn rejects_mixed_or_empty_edit_forms() {
    let edits = [change("old", "new", false)];
    for (old, new, all, batch) in [
        (Some("old"), Some("new"), false, Some(edits.as_slice())),
        (Some("old"), None, false, None),
        (None, Some("new"), false, None),
        (None, None, true, Some(edits.as_slice())),
        (None, None, false, Some(&[][..])),
    ] {
        assert_eq!(
            changes(old, new, all, batch).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
    assert!(
        changes(
            None,
            None,
            false,
            Some(&vec![change("old", "new", true); MAX_FILE_EDITS + 1])
        )
        .is_err()
    );
}

#[test]
fn preserves_large_files_and_crlf() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let prefix = "unchanged 🙂\r\n".repeat(20_000);
    let before = format!("{prefix}old target\r\nlast\r\n");
    std::fs::write(root.path().join("file"), &before).unwrap();
    let written = single(
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
            single(
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
        single(
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
            single(
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
            single(
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
