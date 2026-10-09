use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_unicode_and_reserved_path_bytes() {
    for path in [
        "src/資料+🙂.rs",
        "space path/percent%23.rs",
        "a#b.rs",
        "a?b.rs",
    ] {
        let uri = mapping::uri(path).unwrap();
        assert_eq!(mapping::relative_uri(uri.as_str()).as_deref(), Some(path));
    }
    for value in [
        "file:///tmp/private.rs",
        "sailry-code://worktree/%2Fprivate.rs",
        "sailry-code://other/a.rs",
        "sailry-code://worktree/a.rs#fragment",
    ] {
        assert!(mapping::relative_uri(value).is_none());
    }
}

#[test]
fn rejects_invalid_diagnostic_ranges() {
    let mut location = code::Location {
        path: "main.rs".into(),
        start: code::Position { line: 1, column: 3 },
        end: code::Position { line: 1, column: 9 },
    };
    assert_eq!(
        mapping::range(&location).unwrap().start,
        lsp_types::Position::new(0, 2)
    );
    location.start.line = 0;
    assert!(mapping::range(&location).is_none());
    location.start.line = 2;
    assert!(mapping::range(&location).is_none());
}
