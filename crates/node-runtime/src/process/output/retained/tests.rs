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
    let preview = preview["stdout"]["text"].as_str().unwrap();
    assert!(preview.starts_with("header\n"));
    assert!(preview.ends_with("last failure"));
    assert!(preview.contains("bytes omitted") && preview.contains("read_command"));
    assert!(preview.len() < 34 * 1024);
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
