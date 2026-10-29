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
        background: Vec::new(),
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
fn joins_adjacent_text() {
    let fragments = [
        "摘要 **完",
        "整**\n\n| 列 | 值 |\n| --- | --- |\n| 中文 | 1 |\n\n公式 $x",
        "^2$。\n\n```",
        "mer",
        "maid\nflowchart",
        " LR\nA[用户下单] --",
        "> B[支付]\n",
        "```\n",
    ];
    let source = fragments.concat();
    let page = page(
        fragments
            .into_iter()
            .map(|fragment| Part::Text(fragment.into()))
            .collect(),
    );
    let document = render(&page, "Fragments", Format::Markdown, "assets").unwrap();
    assert_eq!(document.content, format!("## user\n\n{source}\n\n"));
    let document = render(&page, "Fragments", Format::Html, "assets").unwrap();
    assert!(document.content.contains("<strong>完整</strong>"));
    assert!(document.content.contains("<table>"));
    assert!(
        document
            .content
            .contains("<span class=\"math inline\">\\(x^2\\)</span>")
    );
    assert!(document.content.contains(
        "<pre><code class=\"language-mermaid\">flowchart LR\nA[用户下单] --&gt; B[支付]\n</code></pre>"
    ));
}

#[test]
fn separates_attachment_boundaries() {
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
    let name = attachment_name(&attachment);
    let document = render(
        &page(vec![
            Part::Text("Before".into()),
            Part::Attachment(attachment.clone()),
            Part::Text("After".into()),
        ]),
        "Attachment",
        Format::Markdown,
        "assets",
    )
    .unwrap();
    assert_eq!(
        document.attachments.as_slice(),
        std::slice::from_ref(&attachment)
    );
    assert_eq!(
        document.content,
        format!("## user\n\nBefore\n\n[file.txt](<assets/{name}>)\n\nAfter\n\n")
    );
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
fn preserves_reply_before_closing_interactions() {
    for name in ["plan_review", "ask_user", "mcp_elicitation"] {
        let mut page = page(vec![Part::Text("User request".into())]);
        let mut assistant = page.entries[0].clone();
        assistant.id = "assistant".into();
        assistant.author = "assistant".into();
        assistant.sequence = 2;
        assistant.parts = vec![
            Part::Thinking("Work details".into()),
            Part::Text("Final plan".into()),
        ];
        page.entries.push(assistant.clone());
        assistant.id = "closing".into();
        assistant.sequence = 3;
        assistant.parts = vec![
            Part::ToolCall {
                id: Some("closing".into()),
                name: name.into(),
                summary: Default::default(),
                presentation: Default::default(),
                grouping: Default::default(),
            },
            Part::Resource(serde_json::json!({"type":"tool_started","id":"closing","name":name})),
        ];
        page.entries.push(assistant.clone());
        assistant.id = "closed".into();
        assistant.sequence = 4;
        assistant.parts = vec![
            Part::ToolResult {
                id: Some("closing".into()),
                name: name.into(),
                summary: Default::default(),
                images: vec![],
            },
            Part::Resource(serde_json::json!({"type":"provider_context"})),
        ];
        page.entries.push(assistant);
        for format in [Format::Markdown, Format::Html] {
            let document = render(&page, "Plan", format, "assets").unwrap();
            assert!(document.content.contains("Final plan"));
            assert!(!document.content.contains("Work details"));
        }
    }
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

#[test]
fn excludes_notice_input_but_keeps_user_speech() {
    let mut page = page(vec![Part::Text("Node notice fixture".into())]);
    let turn = page.entries[0].turn;
    let run: sailry_protocol::conversation::Run = serde_json::from_value(serde_json::json!({
        "worktree":WorktreeId::new(),"kind":"task","turn":turn,"session":page.session,"sequence":1,
        "revision":1,"status":"completed","notice":true
    }))
    .unwrap();
    page.runs.push(run);
    let mut answer = page.entries[0].clone();
    answer.id = "answer".into();
    answer.sequence = 2;
    answer.author = "assistant".into();
    answer.parts = vec![Part::Text("Actual answer".into())];
    page.entries.push(answer);
    let document = render(&page, "Notices", Format::Markdown, "assets").unwrap();
    assert!(!document.content.contains("Node notice fixture"));
    assert!(document.content.contains("Actual answer"));
    page.runs[0].notice = false;
    let document = render(&page, "User input", Format::Markdown, "assets").unwrap();
    assert!(document.content.contains("Node notice fixture"));
}
