use super::*;
use sailry_link::CancellationToken;
use std::time::{Duration, Instant};

fn directory() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}

fn control() -> Control {
    Control {
        cancelled: CancellationToken::new(),
        deadline: Instant::now() + Duration::from_secs(20),
    }
}

fn options() -> FileSearch {
    FileSearch {
        query: "hit".into(),
        case_sensitive: true,
        ..Default::default()
    }
}

#[test]
fn interrupts_before_traversal() {
    let root = directory();
    let control = control();
    control.cancelled.cancel();
    assert_eq!(
        search(root.path(), &options(), &control).unwrap_err().code,
        ErrorCode::Cancelled
    );
    let control = Control {
        cancelled: CancellationToken::new(),
        deadline: Instant::now(),
    };
    assert_eq!(
        search(root.path(), &options(), &control).unwrap_err().code,
        ErrorCode::Busy
    );
}

#[test]
fn searches_beyond_the_old_total_budget() {
    let root = directory();
    let mut content = "unmatched\n".repeat(1_000_000);
    content.push_str("hit after large input\n");
    std::fs::write(root.path().join("a.txt"), &content).unwrap();
    std::fs::write(root.path().join("b.txt"), &content).unwrap();
    let result = search(root.path(), &options(), &control()).unwrap();
    assert!(!result.partial, "{result:?}");
    assert_eq!(result.matches.len(), 2);
    assert!(
        result
            .matches
            .iter()
            .all(|row| row.line_number == 1_000_001)
    );
    assert_eq!(result.scanned_files, 2);
}

#[test]
fn reports_large_files_and_long_lines() {
    let root = directory();
    std::fs::File::create(root.path().join("large.txt"))
        .unwrap()
        .set_len((MAX_BYTES + 1) as u64)
        .unwrap();
    std::fs::write(
        root.path().join("line.txt"),
        format!("hit {}", "🙂".repeat(MAX_LINE)),
    )
    .unwrap();
    let result = search(root.path(), &options(), &control()).unwrap();
    assert!(result.partial);
    assert!(result.next_offset.is_none());
    assert_eq!(result.matches.len(), 1);
    assert!(result.matches[0].truncated);
    assert!(result.matches[0].line.len() <= MAX_LINE);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|item| item.path == "large.txt" && item.message.contains("64 MiB"))
    );
}

#[test]
fn pages_context_and_distinguishes_modes() {
    let root = directory();
    std::fs::write(
        root.path().join("a.rs"),
        "before\r\nhit\r\nafter\r\nhit\r\nend\r\n",
    )
    .unwrap();
    std::fs::write(root.path().join("b.js"), "hit\nhit\n").unwrap();
    let mut options = options();
    options.context = 1;
    options.file_type = Some("rust".into());
    options.limit = 3;
    let first = search(root.path(), &options, &control()).unwrap();
    assert_eq!(
        first
            .matches
            .iter()
            .map(|row| row.line_number)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        first
            .matches
            .iter()
            .map(|row| row.matched)
            .collect::<Vec<_>>(),
        [false, true, false]
    );
    assert_eq!(first.next_offset, Some(3));
    options.offset = 3;
    let second = search(root.path(), &options, &control()).unwrap();
    assert_eq!(
        second
            .matches
            .iter()
            .map(|row| row.line_number)
            .collect::<Vec<_>>(),
        [4, 5]
    );
    assert_eq!(second.next_offset, None);
    options.offset = 0;
    options.file_type = None;
    options.mode = SearchMode::Files;
    let files = search(root.path(), &options, &control()).unwrap();
    assert_eq!(files.paths, ["a.rs", "b.js"]);
    assert!(files.matches.is_empty() && files.counts.is_empty());
    options.mode = SearchMode::Count;
    let counts = search(root.path(), &options, &control()).unwrap();
    assert_eq!(
        counts
            .counts
            .iter()
            .map(|item| item.count)
            .collect::<Vec<_>>(),
        [2, 2]
    );
    assert!(counts.matches.is_empty() && counts.paths.is_empty());
}

#[test]
fn matches_across_lines_when_requested() {
    let root = directory();
    std::fs::write(root.path().join("a.py"), "before\nfirst\nsecond\nafter\n").unwrap();
    let options = FileSearch {
        query: "first.*second".into(),
        regex: true,
        multiline: true,
        context: 1,
        file_type: Some("python".into()),
        ..Default::default()
    };
    let result = search(root.path(), &options, &control()).unwrap();
    assert_eq!(
        result
            .matches
            .iter()
            .map(|row| row.line.as_str())
            .collect::<Vec<_>>(),
        ["before", "first", "second", "after"]
    );
    assert_eq!(
        result
            .matches
            .iter()
            .map(|row| row.matched)
            .collect::<Vec<_>>(),
        [false, true, true, false]
    );
    let result = search(
        root.path(),
        &FileSearch {
            multiline: false,
            context: 0,
            ..options
        },
        &control(),
    )
    .unwrap();
    assert!(result.matches.is_empty());
}

#[test]
fn rejects_invalid_options() {
    let root = directory();
    for options in [
        FileSearch {
            limit: 0,
            ..options()
        },
        FileSearch {
            limit: 201,
            ..options()
        },
        FileSearch {
            context: 6,
            ..options()
        },
        FileSearch {
            file_type: Some("not-a-language".into()),
            ..options()
        },
        FileSearch {
            path: "../outside".into(),
            ..options()
        },
        FileSearch {
            regex: true,
            query: "[".into(),
            ..options()
        },
    ] {
        assert_eq!(
            search(root.path(), &options, &control()).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}

#[test]
fn detects_changes_between_scan_and_page() {
    let root = directory();
    std::fs::write(root.path().join("a.txt"), "hit\n").unwrap();
    let options = options();
    let matcher = RegexMatcherBuilder::new().build("hit").unwrap();
    let scan = inspect(
        root.path(),
        "a.txt",
        SearchMode::Content,
        1,
        &matcher,
        &mut engine(&options),
        &control(),
    )
    .unwrap()
    .unwrap();
    std::fs::write(root.path().join("a.txt"), "changed\n").unwrap();
    assert_eq!(
        read_page(
            root.path(),
            &scan,
            0..1,
            &matcher,
            &mut engine(&options),
            &control()
        )
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn preserves_literals_and_crlf_anchors() {
    let root = directory();
    std::fs::write(root.path().join("a.txt"), "needle\r\n[needle]\r\n").unwrap();
    let literal = search(
        root.path(),
        &FileSearch {
            query: "[needle]".into(),
            ..Default::default()
        },
        &control(),
    )
    .unwrap();
    assert_eq!(literal.matches.len(), 1);
    assert_eq!(literal.matches[0].line_number, 2);
    let anchored = search(
        root.path(),
        &FileSearch {
            query: "^needle$".into(),
            regex: true,
            ..Default::default()
        },
        &control(),
    )
    .unwrap();
    assert_eq!(anchored.matches.len(), 1);
    assert_eq!(anchored.matches[0].line_number, 1);
}
