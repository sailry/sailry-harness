use super::*;
use contents::{Envelope, escaped};

fn snapshot() -> GitChangeSnapshot {
    GitChangeSnapshot {
        target: GitChangeState::Range {
            base: "a".repeat(40),
            head: "b".repeat(40),
        },
        metadata_revision: "revision".into(),
    }
}

#[test]
fn matches_encoded_change_pages() {
    let mut page = GitChange {
        snapshot: snapshot(),
        path: "src/\"quoted\".rs".into(),
        old_path: None,
        metadata_revision: "metadata".into(),
        revision: "content".into(),
        offset: 0,
        next: None,
        lines: Vec::new(),
        binary: false,
        skip_reason: None,
        first_line: Some("fn main() {".into()),
    };
    let envelope = Envelope::change(&page).unwrap();
    let mut body = 0;
    for (index, text) in ["plain", "tab\tand \"quote\"", "\u{1}control", "中文 🙂", ""]
        .into_iter()
        .enumerate()
    {
        let line = GitChangeLine {
            kind: GitChangeLineKind::Added,
            old_line: None,
            new_line: Some(index as u32 + 1),
            text: text.into(),
        };
        body += encoded(&line).unwrap();
        page.lines.push(line);
        for next in [index + 1, 9, 10, 12_345] {
            page.next = Some(next);
            assert_eq!(
                envelope.lines(body, page.lines.len(), next),
                encoded(&Output::GitChange(page.clone())).unwrap()
            );
        }
    }
}

#[test]
fn matches_encoded_context_pages() {
    let mut page = GitContext {
        snapshot: snapshot(),
        path: "README.md".into(),
        old_path: Some("OLD.md".into()),
        metadata_revision: "metadata".into(),
        revision: "content".into(),
        side: GitContextSide::New,
        offset: 3,
        next: None,
        text: String::new(),
        binary: false,
        skip_reason: None,
    };
    let envelope = Envelope::context(&page).unwrap();
    let mut body = 0;
    for (index, line) in ["first\n", "\"quoted\"\r\n", "\u{7f}\\slash\n", "last"]
        .into_iter()
        .enumerate()
    {
        body += escaped(line).unwrap();
        page.text.push_str(line);
        for next in [index + 4, 100, 1_000_000] {
            page.next = Some(next);
            assert_eq!(
                envelope.text(body, next),
                encoded(&Output::GitContext(page.clone())).unwrap()
            );
        }
    }
}
