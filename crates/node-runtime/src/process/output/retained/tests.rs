use super::*;
use sailry_protocol::{
    SessionId, TurnId,
    process::{Access, Outcome, Status},
};

fn info() -> Info {
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
fn pages_complete_unicode_after_finish() {
    let retained = Retained::default();
    let text = format!("header\n{}\nlast failure", "中文🙂".repeat(20_000));
    for bytes in text.as_bytes().chunks(8191) {
        retained.append(OutputStream::Stdout, bytes);
    }
    retained.finish();
    assert!(retained.stdout.lock().unwrap().writer.is_none());
    let mut offset = 0;
    let mut actual = String::new();
    loop {
        let page = retained
            .page(info(), OutputStream::Stdout, offset, 101)
            .unwrap();
        assert!(!page.truncated && !page.invalid_utf8);
        assert_eq!(page.total_bytes, text.len() as u64);
        actual.push_str(&page.text);
        let Some(next) = page.next_offset else {
            break;
        };
        assert!(next > offset && text.is_char_boundary(next));
        offset = next;
    }
    assert_eq!(actual, text);
    let preview = retained.preview(info().id).unwrap();
    let preview = &preview["stdout"];
    assert!(preview["head"].as_str().unwrap().starts_with("header\n"));
    assert!(preview["tail"].as_str().unwrap().ends_with("last failure"));
    let head = preview["head_bytes"].as_u64().unwrap() as usize;
    let tail = preview["tail_offset"].as_u64().unwrap() as usize;
    assert_eq!(preview["total_bytes"], text.len());
    assert_eq!(preview["head"], &text[..head]);
    assert_eq!(preview["tail"], &text[tail..]);
    assert!(head <= HEAD && text.len() - tail <= TAIL);
}

#[test]
fn pages_large_output_without_truncation() {
    let retained = Retained::default();
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
        assert!(!page.truncated && !page.invalid_utf8);
        assert_eq!(page.total_bytes, total as u64);
        assert!(page.text.len() <= PAGE);
        actual.update(page.text.as_bytes());
        let end = offset + page.text.len();
        if let Some(next) = page.next_offset {
            assert_eq!(next, end);
            assert!(next > offset);
            offset = next;
        } else {
            assert_eq!(end, total);
            break page.text;
        }
    };
    assert!(last.ends_with(std::str::from_utf8(suffix).unwrap()));
    assert_eq!(actual.finalize(), expected.finalize());
    let preview = retained.preview(info().id).unwrap();
    let preview = &preview["stdout"];
    assert!(
        preview["tail"]
            .as_str()
            .unwrap()
            .ends_with(std::str::from_utf8(suffix).unwrap())
    );
    assert_eq!(preview["retained_bytes"], total);
}

#[test]
fn preserves_raw_and_reports_unavailable_retention() {
    let mut stream = Stream::default();
    stream.append(b"\x1b[31m1%\r2%\rDone\x1b[0m\n");
    let original = stream.page(info(), OutputStream::Stdout, 0, 1024).unwrap();
    assert_eq!(original.text, "\x1b[31m1%\r2%\rDone\x1b[0m\n");
    stream.error = Some("fixture write failure".into());
    assert_eq!(
        stream
            .page(info(), OutputStream::Stdout, 0, 100)
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
}

#[test]
fn releases_only_owned_output_files() {
    let retained = Retained::default();
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
    assert!(path.exists());
    drop(retained);
    assert!(!path.exists());
}

#[test]
fn reports_the_retention_limit() {
    let mut stream = Stream::default();
    stream.limit = 12;
    stream.append(b"first bytes ");
    stream.append(b"remaining bytes");
    stream.finish();
    let page = stream.page(info(), OutputStream::Stdout, 0, 1024).unwrap();
    assert_eq!(page.text, "first bytes ");
    assert!(page.truncated);
    assert_eq!(page.total_bytes, 27);
    assert!(page.next_offset.is_none());
    assert_eq!(stream.retained, 12);
    assert_eq!(
        std::fs::metadata(stream.path.as_ref().unwrap())
            .unwrap()
            .len(),
        12
    );
}

#[test]
fn shares_budget_and_preserves_prefixes() {
    let budget = Arc::new(Budget::limited(12));
    let mut first = Stream::new(tempfile::NamedTempFile::new(), budget.clone(), 8);
    let mut second = Stream::new(tempfile::NamedTempFile::new(), budget.clone(), 8);
    assert_eq!(budget.available(), 0);
    first.append(b"12345678");
    second.append(b"abcdef");
    assert_eq!(budget.used.load(Ordering::Relaxed), 12);
    let page = second.page(info(), OutputStream::Stderr, 0, PAGE).unwrap();
    assert_eq!(page.text, "abcd");
    assert!(page.truncated);
    assert_eq!(page.total_bytes, 6);
    drop(first);
    assert_eq!(budget.used.load(Ordering::Relaxed), 4);
    second.append(b"later");
    assert_eq!(
        second
            .page(info(), OutputStream::Stderr, 0, PAGE)
            .unwrap()
            .text,
        "abcd"
    );
    let mut third = Stream::new(tempfile::NamedTempFile::new(), budget.clone(), 8);
    third.append(b"reused");
    third.finish();
    assert_eq!(budget.used.load(Ordering::Relaxed), 10);
    drop(second);
    drop(third);
    assert_eq!(budget.used.load(Ordering::Relaxed), 0);
}

#[test]
fn returns_unused_reservations_on_finish() {
    let profile = tempfile::tempdir().unwrap();
    let budget = Arc::new(Budget::limited(COMMAND_LIMIT));
    let retained = Retained::new(profile.path(), budget.clone());
    assert_eq!(budget.available(), 0);
    retained.append(OutputStream::Stdout, b"saved output");
    retained.finish();
    retained.finish();
    assert_eq!(budget.available(), COMMAND_LIMIT - 12);
    assert_eq!(
        retained
            .page(info(), OutputStream::Stdout, 0, PAGE)
            .unwrap()
            .text,
        "saved output"
    );
}

#[test]
fn reports_raw_size_beyond_stream_quota() {
    let retained = Retained::default();
    let raw = vec![b'x'; STREAM_LIMIT + PAGE];
    retained.append(OutputStream::Stdout, &raw);
    retained.append(OutputStream::Stdout, b"last failure");
    retained.finish();
    let page = retained
        .page(info(), OutputStream::Stdout, STREAM_LIMIT - PAGE, PAGE)
        .unwrap();
    assert!(page.truncated);
    assert_eq!(page.text.len(), PAGE);
    assert!(page.next_offset.is_none());
    assert_eq!(page.total_bytes, (raw.len() + 12) as u64);
    let preview = retained.preview(info().id).unwrap();
    let preview = &preview["stdout"];
    assert_eq!(preview["total_bytes"], page.total_bytes);
    assert_eq!(preview["retained_bytes"], STREAM_LIMIT);
    assert_eq!(preview["head_bytes"], HEAD);
    assert_eq!(preview["tail_offset"], page.total_bytes - TAIL as u64);
    assert!(preview["tail"].as_str().unwrap().ends_with("last failure"));
}

#[test]
fn clears_only_stale_owned_files() {
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
    assert!(!owned.exists());
    assert_eq!(std::fs::read_to_string(unrelated).unwrap(), "preserved");
    assert!(nested.is_dir());
    #[cfg(unix)]
    assert!(std::fs::symlink_metadata(link).is_err());
}
