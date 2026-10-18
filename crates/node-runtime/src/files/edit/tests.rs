use super::*;

#[test]
fn matches_lf_edits_without_normalizing_original_bytes() {
    let before = "prefix\r\nfirst\r\nsecond\r\nkeep\nfirst\nsecond\nend\r\n";
    let edits = [FileEdit {
        old_text: "first\nsecond\n".into(),
        new_text: "changed\nmore\nlast\n".into(),
        replace_all: true,
    }];
    let after = prepare(before, &edits, &revision(before)).unwrap();
    assert_eq!(
        after,
        "prefix\r\nchanged\r\nmore\r\nlast\r\nkeep\nchanged\nmore\nlast\nend\r\n"
    );
    assert_eq!(
        prepare(before, &edits, &revision(&before.replace("\r\n", "\n")))
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    let mut unique = edits;
    unique[0].replace_all = false;
    assert_eq!(
        prepare(before, &unique, &revision(before))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn preserves_mixed_terminators_and_literal_carriage_returns() {
    let before = "keep\r\na\r\nb\nc\r\ntail\n";
    let edits = [
        change("a\nb\nc", "A\nB\nC", false),
        change("tail", "last\nextra", false),
    ];
    assert_eq!(
        prepare(before, &edits, &revision(before)).unwrap(),
        "keep\r\nA\r\nB\nC\r\nlast\nextra\n"
    );
    assert_eq!(
        prepare(
            "a\rb\r\n",
            &[change("\r", "", false)],
            &revision("a\rb\r\n")
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        prepare("a\rb\n", &[change("\r", "", false)], &revision("a\rb\n")).unwrap(),
        "ab\n"
    );
}

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
    for replacement in ["\0".into(), "x".repeat(MAX_EDIT_BYTES + 1)] {
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

#[test]
fn applies_large_batches_and_text_atomically() {
    let before = (0..129)
        .map(|index| format!("unique-{index:04}\n"))
        .collect::<String>();
    let edits = (0..129)
        .map(|index| {
            change(
                &format!("unique-{index:04}\n"),
                &format!("changed-{index:04}\n"),
                false,
            )
        })
        .collect::<Vec<_>>();
    let expected = (0..129)
        .map(|index| format!("changed-{index:04}\n"))
        .collect::<String>();
    assert_eq!(
        prepare(&before, &edits, &revision(&before)).unwrap(),
        expected
    );
    let before = "x".repeat(128 * 1024 + 1);
    let replacement = "y".repeat(128 * 1024 + 1);
    assert_eq!(
        prepare(
            &before,
            &[change(&before, &replacement, false)],
            &revision(&before)
        )
        .unwrap(),
        replacement
    );
}
