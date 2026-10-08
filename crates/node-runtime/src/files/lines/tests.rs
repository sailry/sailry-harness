use super::*;
use sailry_link::CancellationToken;
use std::time::{Duration, Instant};

fn control() -> Control {
    Control {
        cancelled: CancellationToken::new(),
        deadline: Instant::now() + Duration::from_secs(5),
    }
}

#[test]
fn preserves_numbering_and_endings() {
    let root = tempfile::tempdir().unwrap();
    let text = "first\r\nsecond 🙂\nlast";
    std::fs::write(root.path().join("file"), text).unwrap();
    let page = read(
        &root.path().canonicalize().unwrap(),
        "file".into(),
        1,
        1,
        &control(),
    )
    .unwrap();
    assert_eq!(
        page.lines,
        vec![FileLine {
            number: 2,
            text: "second 🙂\n".into()
        }]
    );
    assert_eq!(page.offset, 1);
    assert_eq!(page.next_offset, Some(2));
    assert_eq!(page.size, text.len() as u64);
    assert_eq!(
        page.revision,
        blake3::hash(text.as_bytes()).to_hex().to_string()
    );
    let page = read(
        &root.path().canonicalize().unwrap(),
        "file".into(),
        0,
        3,
        &control(),
    )
    .unwrap();
    assert_eq!(page.lines[0].text, "first\r\n");
    assert_eq!(page.lines[2].text, "last");
    assert!(page.next_offset.is_none());
}

#[test]
fn hashes_the_complete_large_file() {
    let root = tempfile::tempdir().unwrap();
    let text = "large line\r\n".repeat(20_000);
    assert!(text.len() > MAX_FILE_BYTES);
    std::fs::write(root.path().join("file"), &text).unwrap();
    let page = read(
        &root.path().canonicalize().unwrap(),
        "file".into(),
        19_998,
        200,
        &control(),
    )
    .unwrap();
    assert_eq!(page.lines.len(), 2);
    assert_eq!(page.lines[0].number, 19_999);
    assert_eq!(
        page.revision,
        blake3::hash(text.as_bytes()).to_hex().to_string()
    );
    assert!(page.next_offset.is_none());
}

#[test]
fn bounds_pages_without_partial_lines() {
    let root = tempfile::tempdir().unwrap();
    let line = format!("{}\n", "x".repeat(MAX_FILE_BYTES / 2));
    std::fs::write(root.path().join("file"), line.repeat(3)).unwrap();
    let page = read(
        &root.path().canonicalize().unwrap(),
        "file".into(),
        0,
        200,
        &control(),
    )
    .unwrap();
    assert_eq!(page.lines.len(), 1);
    assert_eq!(page.lines[0].text, line);
    assert_eq!(page.next_offset, Some(1));
    std::fs::write(root.path().join("file"), "x".repeat(MAX_FILE_BYTES + 1)).unwrap();
    assert_eq!(
        read(
            &root.path().canonicalize().unwrap(),
            "file".into(),
            0,
            200,
            &control()
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn rejects_invalid_text_and_limits() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    for text in [b"binary\0".as_slice(), &[0xff]] {
        std::fs::write(root.path().join("file"), text).unwrap();
        assert_eq!(
            read(&path, "file".into(), 0, 200, &control())
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    std::fs::write(root.path().join("file"), "").unwrap();
    assert!(
        read(&path, "file".into(), 0, 200, &control())
            .unwrap()
            .lines
            .is_empty()
    );
    for limit in [0, MAX_FILE_LINES + 1] {
        assert_eq!(
            read(&path, "file".into(), 0, limit, &control())
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(root.path().join("file"))
        .unwrap()
        .set_len(MAX_EDIT_BYTES as u64 + 1)
        .unwrap();
    assert_eq!(
        read(&path, "file".into(), 0, 200, &control())
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn observes_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let control = control();
    control.cancelled.cancel();
    assert_eq!(
        read(root.path(), "file".into(), 0, 1, &control)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}
