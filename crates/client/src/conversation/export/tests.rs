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
fn keeps_body_without_thinking() {
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
    assert!(!document.content.contains("quoted"));
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
    assert_eq!(
        document.attachments.as_slice(),
        std::slice::from_ref(&attachment)
    );
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

#[test]
fn excludes_work_before_final_reply() {
    let mut page = page(vec![Part::Text("Question".into())]);
    let mut assistant = page.entries[0].clone();
    assistant.id = "assistant".into();
    assistant.author = "assistant".into();
    assistant.sequence = 2;
    assistant.parts = vec![
        Part::Text("Progress update".into()),
        Part::Thinking("Private reasoning".into()),
        Part::Text("Final **answer**".into()),
    ];
    page.entries.push(assistant);
    let document = render(&page, "Conversation", Format::Markdown, "assets").unwrap();
    assert!(document.content.contains("Question"));
    assert!(document.content.contains("Final **answer**"));
    assert!(!document.content.contains("Progress update"));
    assert!(!document.content.contains("Private reasoning"));
    let document = render(&page, "Conversation", Format::Html, "assets").unwrap();
    assert!(
        document
            .content
            .contains("<section class=\"message user\"><p>Question</p>")
    );
    assert!(
        document
            .content
            .contains("<section class=\"message assistant\"><p>Final <strong>answer</strong>")
    );
    assert!(!document.content.contains("Progress update"));
    assert!(!document.content.contains("Private reasoning"));
}

#[test]
fn preserves_formula_delimiters_and_code() {
    let text = r"Inline \(x^2\) and $y^2$.

\[a < b\]

`\(literal\)`";
    let document = render(
        &page(vec![Part::Text(text.into())]),
        "Formulas",
        Format::Html,
        "assets",
    )
    .unwrap();
    assert!(document.content.contains(r"\(x^2\)"));
    assert!(document.content.contains(r"\(y^2\)"));
    assert!(document.content.contains(r"\[a &lt; b\]"));
    assert!(document.content.contains(r"<code>\(literal\)</code>"));
}
