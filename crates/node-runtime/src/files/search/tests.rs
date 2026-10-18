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
    assert!(!result.partial);
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
    assert_eq!(result.skipped, 1);
}

#[test]
fn pages_past_skipped_files() {
    let root = directory();
    std::fs::File::create(root.path().join("a-large.txt"))
        .unwrap()
        .set_len((MAX_BYTES + 1) as u64)
        .unwrap();
    std::fs::write(root.path().join("b-binary.txt"), b"hit\0binary").unwrap();
    std::fs::write(root.path().join("c-invalid.txt"), b"hit\xff").unwrap();
    std::fs::write(
        root.path().join("d-text.txt"),
        "hit first\nhit second\nhit third\n",
    )
    .unwrap();
    for offset in 0..3 {
        let result = search(
            root.path(),
            &FileSearch {
                offset,
                limit: 1,
                ..options()
            },
            &control(),
        )
        .unwrap();
        assert!(!result.partial, "{result:?}");
        assert_eq!(result.scanned_files, 4);
        assert_eq!(result.skipped, 3);
        assert_eq!(result.diagnostics.len(), 2);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|item| item.path != "b-binary.txt")
        );
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].path, "d-text.txt");
        assert_eq!(result.matches[0].line_number, offset + 1);
        assert_eq!(result.next_offset, (offset < 2).then_some(offset + 1));
        assert_eq!(result.truncated, offset < 2);
    }
}

#[test]
fn binary_skips_do_not_consume_diagnostics() {
    let root = directory();
    for index in 0..100 {
        std::fs::write(
            root.path().join(format!("binary-{index:03}")),
            b"hit\0binary",
        )
        .unwrap();
    }
    std::fs::write(root.path().join("invalid.txt"), b"hit\xff").unwrap();
    std::fs::write(root.path().join("text.txt"), "hit\n").unwrap();
    let result = search(root.path(), &options(), &control()).unwrap();
    assert_eq!(result.skipped, 101);
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].path, "invalid.txt");
    assert!(!result.partial);
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].path, "text.txt");
}

#[test]
fn decodes_text_before_matching_and_paging() {
    let root = directory();
    std::fs::write(
        root.path().join("a-gbk.txt"),
        b"\xd6\xd0\xce\xc4 first\r\n\xd6\xd0\xce\xc4 second\r\n",
    )
    .unwrap();
    let original = "中文𠀀🙂\n";
    let (encoded, _, failed) = encoding_rs::GB18030.encode(original);
    assert!(!failed);
    std::fs::write(root.path().join("b-gb18030.txt"), &encoded).unwrap();
    for (offset, expected, line) in [
        (0, "中文 first", 1),
        (1, "中文 second", 2),
        (2, "中文𠀀🙂", 1),
    ] {
        let result = search(
            root.path(),
            &FileSearch {
                query: "中文".into(),
                offset,
                limit: 1,
                ..options()
            },
            &control(),
        )
        .unwrap();
        assert!(!result.partial);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].line, expected);
        assert_eq!(result.matches[0].line_number, line);
        assert_eq!(result.next_offset, (offset < 2).then_some(offset + 1));
    }
    let anchored = search(
        root.path(),
        &FileSearch {
            query: "^中文.*second$".into(),
            regex: true,
            ..options()
        },
        &control(),
    )
    .unwrap();
    assert_eq!(anchored.matches.len(), 1);
    assert_eq!(anchored.matches[0].line, "中文 second");
    assert_eq!(anchored.matches[0].line_number, 2);
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
fn preserves_large_context_across_bounded_pages() {
    let root = directory();
    let text = (1..=401)
        .map(|line| {
            if line == 201 {
                "hit\n".into()
            } else {
                format!("line {line}\n")
            }
        })
        .collect::<String>();
    std::fs::write(root.path().join("file.txt"), text).unwrap();
    for context in [6, 8] {
        let page = search(
            root.path(),
            &FileSearch {
                context,
                limit: 1_000_000,
                ..options()
            },
            &control(),
        )
        .unwrap();
        assert_eq!(page.matches.len(), 2 * context + 1);
        assert_eq!(page.matches.first().unwrap().line_number, 201 - context);
        assert_eq!(page.matches.last().unwrap().line_number, 201 + context);
        assert_eq!(page.matches.iter().filter(|row| row.matched).count(), 1);
        assert!(page.next_offset.is_none());
    }
    let options = FileSearch {
        context: 1000,
        limit: 1_000_000,
        ..options()
    };
    let mut offset = 0;
    let mut lines = Vec::new();
    loop {
        let page = search(
            root.path(),
            &FileSearch {
                offset,
                ..options.clone()
            },
            &control(),
        )
        .unwrap();
        assert!(page.matches.len() <= MAX_SEARCH_MATCHES);
        lines.extend(page.matches.iter().map(|row| row.line_number));
        let Some(next) = page.next_offset else { break };
        assert_eq!(next, offset + page.matches.len());
        offset = next;
    }
    assert_eq!(lines, (1..=401).collect::<Vec<_>>());
}

#[test]
fn accepts_long_queries_without_changing_the_pattern() {
    let root = directory();
    let query = "x".repeat(4097);
    std::fs::write(root.path().join("file.txt"), format!("{query}\n")).unwrap();
    let result = search(
        root.path(),
        &FileSearch {
            query: query.clone(),
            ..options()
        },
        &control(),
    )
    .unwrap();
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].line, query);
    assert!(result.matches[0].matched);
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
fn detects_encoding_changes_before_paging() {
    let root = directory();
    std::fs::write(root.path().join("a.txt"), b"hit \xd6\xd0\xce\xc4\n").unwrap();
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
    std::fs::write(root.path().join("a.txt"), "hit 中文\n").unwrap();
    assert_eq!(
        read_page(
            root.path(),
            &scan,
            0..1,
            &matcher,
            &mut engine(&options),
            &control(),
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
