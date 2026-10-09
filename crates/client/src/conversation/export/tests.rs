use super::*;
use sailry_protocol::{AttachmentId, TurnId, WorktreeId, attachment::Spec, conversation::Entry};

fn page(parts: Vec<Part>) -> Page {
    Page {
        session: SessionId::new(),
        revision: 1,
        entries: vec![Entry {
            sequence: 1,
            id: "message".into(),
            turn: TurnId::new(),
            author: "user".into(),
            branch: String::new(),
            timestamp_ms: 1,
            parts,
            citations: vec![],
            search_suggestions: None,
            usage: None,
        }],
        runs: vec![],
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        next_before: None,
    }
}

#[test]
fn keeps_complete_unicode_and_safe_fences() {
    let text = "完整文本 🙂\n```rust\nlet example = 1;\n```\n".repeat(400);
    let document = render(
        &page(vec![
            Part::Text(text.clone()),
            Part::Thinking("``` quoted".into()),
        ]),
        "Conversation",
        Format::Markdown,
        "assets",
    )
    .unwrap();
    assert!(document.content.contains(&text));
    assert!(document.content.contains("````text\n``` quoted\n````"));
}

#[test]
fn escapes_html_and_script_links() {
    let document = render(
        &page(vec![Part::Text(
            "<script>alert(1)</script>\n[link](javascript:alert) ![image](data:text/html,bad)"
                .into(),
        )]),
        "<bad>",
        Format::Html,
        "assets",
    )
    .unwrap();
    assert!(document.content.contains("&lt;script&gt;"));
    assert!(!document.content.contains("<script>"));
    assert!(!document.content.contains("href=\"javascript:"));
    assert!(!document.content.contains("src=\"data:"));
    assert!(document.content.contains("<title>&lt;bad&gt;</title>"));
}

#[test]
fn deduplicates_attachments_without_path_escapes() {
    let attachment = Attachment {
        id: AttachmentId::new(),
        spec: Spec {
            worktree: WorktreeId::new(),
            name: "../中文 image.png".into(),
            media_type: "image/png".into(),
            size: 3,
            revision: blake3::hash(b"img").to_hex().to_string(),
        },
    };
    let document = render(
        &page(vec![
            Part::Attachment(attachment.clone()),
            Part::Attachment(attachment.clone()),
        ]),
        "Export",
        Format::Markdown,
        "export-assets",
    )
    .unwrap();
    assert_eq!(document.attachments, [attachment.clone()]);
    assert!(!attachment_name(&attachment).contains('/'));
    assert!(document.content.contains("export-assets/"));
    assert!(render(&page(vec![]), "Export", Format::Markdown, "../assets").is_err());
}

#[test]
fn rejects_changed_attachment_metadata() {
    let attachment = Attachment {
        id: AttachmentId::new(),
        spec: Spec {
            worktree: WorktreeId::new(),
            name: "file.txt".into(),
            media_type: "text/plain".into(),
            size: 3,
            revision: blake3::hash(b"txt").to_hex().to_string(),
        },
    };
    let mut changed = attachment.clone();
    changed.spec.size += 1;
    assert!(
        render(
            &page(vec![
                Part::Attachment(attachment),
                Part::Attachment(changed)
            ]),
            "Export",
            Format::Markdown,
            "assets"
        )
        .is_err()
    );
}
