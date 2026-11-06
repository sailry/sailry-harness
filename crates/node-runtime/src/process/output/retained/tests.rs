use super::*;
use sailry_protocol::{
    SessionId, TurnId,
    process::{Access, Outcome, Status},
};

pub(super) fn info() -> Info {
    Info {
        id: RequestId::new(),
        session: SessionId::new(),
        turn: TurnId::new(),
        command: "fixture".into(),
        access: Access::Workspace,
        pty: false,
        cwd: String::new(),
        started_ms: 0,
        status: Status::Finished(Outcome::Exited(0)),
        services: Vec::new(),
    }
}

#[test]
fn pages_finished_output_at_raw_offsets() {
    for (case_name, run) in [
        (
            "pages_complete_unicode_after_finish",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let retained = Retained::new(profile.path(), Arc::default());
                let text = format!("header\n{}\nlast failure", "中文🙂".repeat(20_000));
                for bytes in text.as_bytes().chunks(8191) {
                    retained.append(OutputStream::Stdout, bytes);
                }
                retained.finish();
                assert!(
                    retained.stdout.lock().unwrap().writer.is_none(),
                    "{case_name}"
                );
                let mut offset = 0;
                let mut actual = String::new();
                loop {
                    let page = retained
                        .page(info(), OutputStream::Stdout, offset, 101)
                        .unwrap();
                    assert!(!page.truncated && !page.invalid_utf8, "{case_name}");
                    assert_eq!(page.total_bytes, text.len() as u64, "{case_name}");
                    actual.push_str(&page.text);
                    let Some(next) = page.next_offset else {
                        break;
                    };
                    assert!(next > offset && text.is_char_boundary(next), "{case_name}");
                    offset = next;
                }
                assert_eq!(actual, text, "{case_name}");
                let preview = retained.preview(info().id).unwrap();
                let preview = &preview["stdout"];
                assert!(
                    preview["tail"].as_str().unwrap().ends_with("last failure"),
                    "{case_name}"
                );
                let tail = preview["tail_offset"].as_u64().unwrap() as usize;
                assert_eq!(preview["total_bytes"], text.len(), "{case_name}");
                assert!(preview.get("head").is_none(), "{case_name}");
                assert_eq!(preview["tail"], &text[tail..], "{case_name}");
                assert!(text.len() - tail <= TAIL, "{case_name}");
            }) as fn(&str),
        ),
        (
            "pages_large_output_without_truncation",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let retained = Retained::new(profile.path(), Arc::default());
                let chunk = vec![b'x'; PAGE];
                let mut expected = blake3::Hasher::new();
                let mut total = 0;
                for _ in 0..=(2 * 1024 * 1024 / PAGE) {
                    retained.append(OutputStream::Stdout, &chunk);
                    expected.update(&chunk);
                    total += chunk.len();
                }
                let suffix = b"\nlast failure\n";
                retained.append(OutputStream::Stdout, suffix);
                expected.update(suffix);
                total += suffix.len();
                retained.finish();
                let mut actual = blake3::Hasher::new();
                let mut offset = 0;
                let last = loop {
                    let page = retained
                        .page(info(), OutputStream::Stdout, offset, PAGE)
                        .unwrap();
                    assert!(!page.truncated && !page.invalid_utf8, "{case_name}");
                    assert_eq!(page.total_bytes, total as u64, "{case_name}");
                    assert!(page.text.len() <= PAGE, "{case_name}");
                    actual.update(page.text.as_bytes());
                    let end = offset + page.text.len();
                    if let Some(next) = page.next_offset {
                        assert_eq!(next, end, "{case_name}");
                        assert!(next > offset, "{case_name}");
                        offset = next;
                    } else {
                        assert_eq!(end, total, "{case_name}");
                        break page.text;
                    }
                };
                assert!(
                    last.ends_with(std::str::from_utf8(suffix).unwrap()),
                    "{case_name}"
                );
                assert_eq!(actual.finalize(), expected.finalize(), "{case_name}");
                let preview = retained.preview(info().id).unwrap();
                let preview = &preview["stdout"];
                assert!(
                    preview["tail"]
                        .as_str()
                        .unwrap()
                        .ends_with(std::str::from_utf8(suffix).unwrap()),
                    "{case_name}"
                );
                assert_eq!(preview["retained_bytes"], total, "{case_name}");
            }) as fn(&str),
        ),
        (
            "pages_rolling_tail_at_original_offsets",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let retained = Retained::new(profile.path(), Arc::default());
                let prefix = b"test suite started\n";
                retained.append(OutputStream::Stdout, prefix);
                let logs = vec![b'x'; 20 * 1024 * 1024];
                for chunk in logs.chunks(8191) {
                    retained.append(OutputStream::Stdout, chunk);
                }
                let failures = format!(
                    "\nfailures:\n{}last failure\n",
                    "failed assertion 中文🙂\n".repeat(2000)
                );
                assert!(failures.len() > 30 * 1024, "{case_name}");
                let start = prefix.len() + logs.len();
                retained.append(OutputStream::Stdout, failures.as_bytes());
                retained.finish();
                assert_eq!(
                    retained
                        .page(info(), OutputStream::Stdout, 0, prefix.len())
                        .unwrap()
                        .text
                        .as_bytes(),
                    prefix,
                    "{case_name}"
                );
                let mut offset = start;
                let mut actual = String::new();
                loop {
                    let page = retained
                        .page(info(), OutputStream::Stdout, offset, 4093)
                        .unwrap();
                    assert_eq!(page.offset, offset, "{case_name}");
                    assert!(page.truncated && !page.invalid_utf8, "{case_name}");
                    assert_eq!(
                        page.total_bytes,
                        (start + failures.len()) as u64,
                        "{case_name}"
                    );
                    actual.push_str(&page.text);
                    match page.next_offset {
                        Some(next) => {
                            assert!(next > offset, "{case_name}");
                            offset = next;
                        }
                        None => break,
                    }
                }
                assert_eq!(actual, failures, "{case_name}");
                let preview = retained.preview(info().id).unwrap();
                let tail = preview["stdout"]["retained_tail_offset"].as_u64().unwrap() as usize;
                let page = retained
                    .page(info(), OutputStream::Stdout, PREFIX_LIMIT - 100, 100)
                    .unwrap();
                assert_eq!(page.next_offset, Some(tail), "{case_name}");
                assert_eq!(
                    retained
                        .page(info(), OutputStream::Stdout, PREFIX_LIMIT + 1, 100)
                        .unwrap()
                        .offset,
                    tail,
                    "{case_name}"
                );
                assert_eq!(
                    retained.stdout.lock().unwrap().retained,
                    STREAM_LIMIT,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "pages_unicode_across_the_retention_gap",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let retained = Retained::new(profile.path(), Arc::default());
                let text = format!("x{}", "中文🙂".repeat(500_000));
                for bytes in text.as_bytes().chunks(8191) {
                    retained.append(OutputStream::Stdout, bytes);
                }
                retained.finish();
                let mut head = PREFIX_LIMIT;
                while !text.is_char_boundary(head) {
                    head -= 1;
                }
                let mut tail = text.len() - (STREAM_LIMIT - PREFIX_LIMIT);
                while !text.is_char_boundary(tail) {
                    tail += 1;
                }
                let preview = retained.preview(info().id).unwrap();
                assert_eq!(
                    preview["stdout"]["retained_head_bytes"], head,
                    "{case_name}"
                );
                assert_eq!(
                    preview["stdout"]["retained_tail_offset"], tail,
                    "{case_name}"
                );
                let mut actual = String::new();
                let mut offset = 0;
                let mut jumped = false;
                loop {
                    let page = retained
                        .page(info(), OutputStream::Stdout, offset, PAGE)
                        .unwrap();
                    assert!(!page.invalid_utf8, "{case_name}");
                    actual.push_str(&page.text);
                    let Some(next) = page.next_offset else {
                        break;
                    };
                    if offset + page.text.len() == head {
                        assert_eq!(next, tail, "{case_name}");
                        jumped = true;
                    }
                    offset = next;
                }
                assert!(jumped, "{case_name}");
                assert_eq!(
                    actual,
                    format!("{}{}", &text[..head], &text[tail..]),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn retains_honest_size_and_budget() {
    for (case_name, run) in [
        (
            "preserves_raw_and_reports_unavailable_retention",
            (|case_name| {
                let mut stream =
                    Stream::new(tempfile::NamedTempFile::new(), Arc::default(), STREAM_LIMIT);
                stream.append(b"\x1b[31m1%\r2%\rDone\x1b[0m\n");
                let original = stream.page(info(), OutputStream::Stdout, 0, 1024).unwrap();
                assert_eq!(
                    original.text, "\x1b[31m1%\r2%\rDone\x1b[0m\n",
                    "{case_name}"
                );
                stream.error = Some("fixture write failure".into());
                assert_eq!(
                    stream
                        .page(info(), OutputStream::Stdout, 0, 100)
                        .unwrap_err()
                        .code,
                    ErrorCode::Unavailable,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "reports_the_retention_limit",
            (|case_name| {
                let mut stream =
                    Stream::new(tempfile::NamedTempFile::new(), Arc::default(), STREAM_LIMIT);
                stream.limit = 12;
                stream.append(b"first bytes ");
                stream.append(b"remaining bytes");
                stream.finish();
                let page = stream.page(info(), OutputStream::Stdout, 0, 1024).unwrap();
                assert_eq!(page.text, "first bytes ", "{case_name}");
                assert!(page.truncated, "{case_name}");
                assert_eq!(page.total_bytes, 27, "{case_name}");
                assert!(page.next_offset.is_none(), "{case_name}");
                assert_eq!(stream.retained, 12, "{case_name}");
                assert_eq!(
                    std::fs::metadata(stream.path.as_ref().unwrap())
                        .unwrap()
                        .len(),
                    12,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "shares_budget_and_preserves_prefixes",
            (|case_name| {
                let budget = Arc::new(Budget::limited(12));
                let mut first = Stream::new(tempfile::NamedTempFile::new(), budget.clone(), 8);
                let mut second = Stream::new(tempfile::NamedTempFile::new(), budget.clone(), 8);
                assert_eq!(budget.available(), 0, "{case_name}");
                first.append(b"12345678");
                second.append(b"abcdef");
                assert_eq!(budget.used.load(Ordering::Relaxed), 12, "{case_name}");
                let page = second.page(info(), OutputStream::Stderr, 0, PAGE).unwrap();
                assert_eq!(page.text, "abcd", "{case_name}");
                assert!(page.truncated, "{case_name}");
                assert_eq!(page.total_bytes, 6, "{case_name}");
                drop(first);
                assert_eq!(budget.used.load(Ordering::Relaxed), 4, "{case_name}");
                second.append(b"later");
                assert_eq!(
                    second
                        .page(info(), OutputStream::Stderr, 0, PAGE)
                        .unwrap()
                        .text,
                    "abcd",
                    "{case_name}"
                );
                let mut third = Stream::new(tempfile::NamedTempFile::new(), budget.clone(), 8);
                third.append(b"reused");
                third.finish();
                assert_eq!(budget.used.load(Ordering::Relaxed), 10, "{case_name}");
                drop(second);
                drop(third);
                assert_eq!(budget.used.load(Ordering::Relaxed), 0, "{case_name}");
            }) as fn(&str),
        ),
        (
            "returns_unused_reservations_on_finish",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let budget = Arc::new(Budget::limited(COMMAND_LIMIT));
                let retained = Retained::new(profile.path(), budget.clone());
                assert_eq!(budget.available(), 0, "{case_name}");
                retained.append(OutputStream::Stdout, b"saved output");
                retained.finish();
                retained.finish();
                assert_eq!(budget.available(), COMMAND_LIMIT - 12, "{case_name}");
                assert_eq!(
                    retained
                        .page(info(), OutputStream::Stdout, 0, PAGE)
                        .unwrap()
                        .text,
                    "saved output",
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "reports_raw_size_beyond_stream_quota",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let retained = Retained::new(profile.path(), Arc::default());
                let raw = vec![b'x'; STREAM_LIMIT + PAGE];
                retained.append(OutputStream::Stdout, &raw);
                retained.append(OutputStream::Stdout, b"last failure");
                retained.finish();
                let page = retained
                    .page(info(), OutputStream::Stdout, raw.len() + 12 - PAGE, PAGE)
                    .unwrap();
                assert!(page.truncated, "{case_name}");
                assert_eq!(page.text.len(), PAGE, "{case_name}");
                assert!(page.next_offset.is_none(), "{case_name}");
                assert_eq!(page.total_bytes, (raw.len() + 12) as u64, "{case_name}");
                let preview = retained.preview(info().id).unwrap();
                let preview = &preview["stdout"];
                assert_eq!(preview["total_bytes"], page.total_bytes, "{case_name}");
                assert_eq!(preview["retained_bytes"], STREAM_LIMIT, "{case_name}");
                assert_eq!(preview["retained_head_bytes"], PREFIX_LIMIT, "{case_name}");
                assert_eq!(
                    preview["tail_offset"],
                    page.total_bytes - TAIL as u64,
                    "{case_name}"
                );
                assert!(
                    preview["tail"].as_str().unwrap().ends_with("last failure"),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn cleans_only_owned_output() {
    for (case_name, run) in [
        (
            "releases_only_owned_output_files",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let retained = Retained::new(profile.path(), Arc::default());
                retained.append(OutputStream::Stdout, b"original");
                retained.finish();
                let path = retained
                    .stdout
                    .lock()
                    .unwrap()
                    .path
                    .as_ref()
                    .unwrap()
                    .to_path_buf();
                assert!(path.exists(), "{case_name}");
                drop(retained);
                assert!(!path.exists(), "{case_name}");
            }) as fn(&str),
        ),
        (
            "clears_only_stale_owned_files",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                clear(profile.path()).unwrap();
                let directory = profile.path().join("tmp");
                std::fs::create_dir(&directory).unwrap();
                let owned = directory.join("command-output-stale");
                let unrelated = directory.join("other-resource");
                let nested = directory.join("command-output-directory");
                std::fs::write(&owned, "stale").unwrap();
                std::fs::write(&unrelated, "preserved").unwrap();
                std::fs::create_dir(&nested).unwrap();
                #[cfg(unix)]
                let link = {
                    let path = directory.join("command-output-link");
                    std::os::unix::fs::symlink(&unrelated, &path).unwrap();
                    path
                };
                clear(profile.path()).unwrap();
                assert!(!owned.exists(), "{case_name}");
                assert_eq!(
                    std::fs::read_to_string(unrelated).unwrap(),
                    "preserved",
                    "{case_name}"
                );
                assert!(nested.is_dir(), "{case_name}");
                #[cfg(unix)]
                assert!(std::fs::symlink_metadata(link).is_err(), "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn preserves_finished_page_identity() {
    for (case_name, run) in [
        (
            "late_bytes_preserve_finished_pages",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let retained = Retained::new(profile.path(), Arc::default());
                retained.append(OutputStream::Stdout, b"saved evidence");
                retained.finish();
                retained.append(OutputStream::Stdout, &vec![b'x'; 70 * 1024]);
                let page = retained
                    .page(info(), OutputStream::Stdout, 0, PAGE)
                    .unwrap();
                assert_eq!(page.text, "saved evidence", "{case_name}");
                assert!(page.truncated, "{case_name}");
                assert_eq!(page.total_bytes, 14 + 70 * 1024, "{case_name}");
                assert_eq!(
                    retained.preview(info().id).unwrap()["stdout"]["retained_end"],
                    14,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "resumes_stale_cursors_across_finish",
            (|case_name| {
                let profile = tempfile::tempdir().unwrap();
                let retained = Retained::new(profile.path(), Arc::default());
                retained.append(OutputStream::Stdout, &vec![b'x'; STREAM_LIMIT + 70 * 1024]);
                let page = retained
                    .page(info(), OutputStream::Stdout, PREFIX_LIMIT - 100, 100)
                    .unwrap();
                let stale = page.next_offset.unwrap();
                retained.append(OutputStream::Stdout, &vec![b'y'; PAGE * 2]);
                let current = retained.preview(info().id).unwrap()["stdout"]["retained_tail_offset"]
                    .as_u64()
                    .unwrap() as usize;
                assert!(stale < current && stale > PREFIX_LIMIT, "{case_name}");
                let resumed = retained
                    .page(info(), OutputStream::Stdout, stale, PAGE)
                    .unwrap();
                assert_eq!(resumed.offset, current, "{case_name}");
                assert_eq!(resumed.text.len(), PAGE, "{case_name}");
                assert_eq!(resumed.text, "x".repeat(PAGE), "{case_name}");
                assert_eq!(resumed.next_offset, Some(current + PAGE), "{case_name}");
                retained.finish();
                let finished = retained
                    .page(info(), OutputStream::Stdout, stale, PAGE)
                    .unwrap();
                assert_eq!(finished.offset, resumed.offset, "{case_name}");
                assert_eq!(finished.text, resumed.text, "{case_name}");
                assert_eq!(finished.next_offset, resumed.next_offset, "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
