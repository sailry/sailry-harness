use super::*;

fn page(
    fixture: &Fixture,
    scope: &GitChanges,
    revision: &str,
    side: GitContextSide,
    offset: usize,
) -> Result<GitContext, Fault> {
    context(
        &fixture.root(),
        scope.snapshot.clone(),
        "file.txt",
        &scope
            .entries
            .iter()
            .find(|entry| entry.path == "file.txt")
            .unwrap()
            .metadata_revision,
        side,
        Page {
            expected_revision: Some(revision),
            offset,
            limit: 1,
        },
        &fixture.control,
    )
}

#[test]
fn preserves_index_sides_and_line_endings() {
    let fixture = Fixture::new();
    let original = "first\r\nold\nlast";
    let staged = "first\r\nindex\nlast-index";
    let working = "first\r\nworking\nlast-working";
    fixture.write("file.txt", original);
    fixture.commit();
    fixture.write("file.txt", staged);
    let mut index = fixture.repository.index().unwrap();
    index.add_path(Path::new("file.txt")).unwrap();
    index.write().unwrap();
    fixture.write("file.txt", working);
    for (scope, old, new) in [
        (GitDiffScope::Staged, original, staged),
        (GitDiffScope::Unstaged, staged, working),
    ] {
        let scope = list(
            &fixture.root(),
            Some(GitChangeTarget::Worktree { scope }),
            None,
            100,
            &fixture.control,
        )
        .unwrap();
        let diff = fixture.read(&scope, "file.txt", None, 0, 100).unwrap();
        for (side, expected) in [(GitContextSide::Old, old), (GitContextSide::New, new)] {
            let mut text = String::new();
            let mut offset = 0;
            loop {
                let current = page(&fixture, &scope, &diff.revision, side, offset).unwrap();
                assert_eq!(current.snapshot, scope.snapshot);
                assert_eq!(current.revision, diff.revision);
                assert_eq!(current.side, side);
                assert_eq!(current.offset, offset);
                assert!(!current.binary);
                assert!(current.skip_reason.is_none());
                assert!(encoded(&Output::GitContext(current.clone())).unwrap() <= MAX_DIFF_BYTES);
                text.push_str(&current.text);
                let Some(next) = current.next else { break };
                assert_eq!(next, offset + 1);
                offset = next;
            }
            assert_eq!(text, expected);
            assert_eq!(
                page(&fixture, &scope, &diff.revision, side, 4)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(
            page(&fixture, &scope, &"0".repeat(64), GitContextSide::New, 0)
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
}

#[test]
fn keeps_live_guards_and_explicit_skips() {
    let fixture = Fixture::new();
    fixture.write("file.txt", "base\n");
    fixture.commit();
    for (bytes, reason, binary) in [
        (b"binary\0payload".to_vec(), "binary", true),
        (vec![b'x'; MAX_DIFF_BYTES * 2], "line_too_large", false),
    ] {
        fixture.write("file.txt", bytes);
        let scope = fixture.working(100);
        let diff = fixture.read(&scope, "file.txt", None, 0, 100).unwrap();
        let current = page(&fixture, &scope, &diff.revision, GitContextSide::New, 0).unwrap();
        assert_eq!(current.skip_reason.as_deref(), Some(reason));
        assert_eq!(current.binary, binary);
        assert!(current.text.is_empty());
        assert!(current.next.is_none());
    }
    fixture.write("file.txt", "changed\n");
    let scope = fixture.working(100);
    let diff = fixture.read(&scope, "file.txt", None, 0, 100).unwrap();
    fixture.write("other.txt", "concurrent change");
    assert_eq!(
        page(&fixture, &scope, &diff.revision, GitContextSide::New, 0)
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}
