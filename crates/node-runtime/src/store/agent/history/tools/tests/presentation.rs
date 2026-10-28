use super::*;
use sailry_protocol::{AttachmentId, WorktreeId, attachment, conversation::Image, tool::Block};

fn source(result: Value) -> Raw {
    let mut raw = Raw::new(
        Some("call".into()),
        "read_file".into(),
        json!({"path":"source.rs"}),
    );
    raw.presentation = Presentation::Content;
    raw.result = Some(result);
    raw
}

#[test]
fn content_requires_canonical_images_and_valid_blocks() {
    let original = json!({"output":{"sailry_content":{"version":1,"blocks":[
        {"kind":"table","columns":["Name"],"rows":[["中文 🙂"]]},
        {"kind":"image","index":2}
    ]}}});
    let mut raw = source(original.clone());
    assert!(raw.detail().content.is_none());
    raw.images.push(Image {
        entry: "event".into(),
        part: 0,
        index: 2,
        attachment: attachment::Attachment {
            id: AttachmentId::new(),
            spec: attachment::Spec {
                worktree: WorktreeId::new(),
                name: "image.png".into(),
                media_type: "image/png".into(),
                size: 5,
                revision: "a".repeat(64),
            },
        },
    });
    assert_eq!(raw.detail().content.unwrap().blocks.len(), 2);
    assert!(raw.summary().has_details);
    raw.failure = Some(Failure { code: None });
    assert!(raw.detail().content.is_none());
    assert!(raw.summary().diagnostics);
    raw.failure = None;
    raw.result.as_mut().unwrap()["output"]["sailry_content"]["version"] = json!(2);
    assert!(raw.detail().content.is_none());
    assert_eq!(original["output"]["sailry_content"]["version"], 1);
}

#[test]
fn publishes_both_artifact_kinds_without_eager_content() {
    for kind in [
        sailry_protocol::artifact::Kind::Document,
        sailry_protocol::artifact::Kind::Preview,
    ] {
        for wrapped in [false, true] {
            let artifact = sailry_protocol::artifact::Summary {
                id: sailry_protocol::ArtifactId::new(),
                kind,
                session: SessionId::new(),
                turn: TurnId::new(),
                package: sailry_protocol::plugin::Reference {
                    name: "document-fixture".into(),
                    digest: "a".repeat(64),
                    settings_revision: 0,
                },
                title: "Options".into(),
                summary: "Compare approaches".into(),
                revision: 1,
                size: 12,
                created_at: 10,
                updated_at: 10,
            };
            let content = sailry_protocol::tool::Content {
                version: 1,
                blocks: vec![Block::Artifact { artifact }],
            };
            let native = json!({"sailry_content":content,"original":"retained"});
            let original = if wrapped {
                json!({"output":native})
            } else {
                native
            };
            let raw = source(original.clone());
            assert!(raw.summary().published);
            assert!(
                !serde_json::to_string(&raw.summary())
                    .unwrap()
                    .contains("Compare approaches")
            );
            assert_eq!(raw.detail().content, Some(content));
            assert_eq!(raw.detail().result, Some(original));
        }
    }
}

#[test]
fn published_files_require_inline_content_for_disclosure() {
    let file = json!({"kind":"file","path":"index.html","mime":"text/html"});
    let mut raw =
        source(json!({"sailry_content":{"version":1,"blocks":[file]},"text":"Original HTML"}));
    assert!(raw.summary().published);
    assert!(!raw.summary().has_details);
    assert!(raw.detail().content.is_some());
    raw.result.as_mut().unwrap()["sailry_content"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"text","path":"/text"}));
    assert!(raw.summary().published);
    assert!(raw.summary().has_details);
    assert_eq!(
        raw.detail().result.as_ref().unwrap()["text"],
        "Original HTML"
    );
}

#[test]
fn preserves_failed_card_receipts() {
    let card = sailry_protocol::plugin::card::Summary {
        receipt: sailry_protocol::plugin::card::Receipt {
            session: SessionId::new(),
            turn: TurnId::new(),
            call: RequestId::new(),
            block: 0,
        },
        node: sailry_protocol::NodeId([1; 32]),
        worktree: WorktreeId::new(),
        package: sailry_protocol::plugin::Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        key: "connection".into(),
        props: json!({"configured":false}),
        fallback: serde_json::from_value(json!({"label":"Connect"})).unwrap(),
    };
    let original = json!({"isError":true,"error":{"code":"not_configured","message":"Connect"},
        "sailry_content":{"version":1,"blocks":[Block::Card(card.clone())]}});
    let mut raw = source(original.clone());
    raw.failure = Some(Failure {
        code: Some(ErrorCode::NotConfigured),
    });
    assert!(raw.summary().published);
    assert_eq!(
        raw.detail().content.unwrap().blocks,
        vec![Block::Card(card)]
    );
    assert_eq!(raw.detail().result, Some(original));
}

#[test]
fn progress_requires_matching_valid_arguments() {
    let arguments = json!({"title":"Task 中文 🙂","steps":[
        {"description":"Inspect","state":"completed"},{"description":"Verify","state":"in_progress"}
    ]});
    let mut raw = source(json!({"progress":arguments}));
    raw.arguments = arguments;
    raw.presentation = Presentation::Progress;
    assert_eq!(raw.summary().progress.unwrap().steps.len(), 2);
    assert!(raw.summary().has_details);
    raw.result.as_mut().unwrap()["progress"]["steps"][0]["state"] = json!("pending");
    assert!(raw.summary().progress.is_none());
    raw.result = Some(json!({"progress":raw.arguments,"error":"not recorded"}));
    assert!(raw.summary().progress.is_none());
    raw.presentation = Presentation::Details;
    assert!(raw.summary().progress.is_none());
}

#[test]
fn presents_mcp_content_without_changing_raw_values() {
    let original = json!({
        "content":[{"type":"text","text":"Original MCP text"}],
        "structuredContent":{
            "text":"Literal stream 中文 🙂",
            "sailry_content":{"version":1,"blocks":[{"kind":"text","path":"/text"}]},
            "sailry_result":{"version":1,"diagnostics":[{"text":"Partial","error":false}]}
        },
        "_meta":{"revision":"original"}
    });
    let mut raw = source(original.clone());
    raw.format = ToolFormat::Mcp;
    raw.display = Some(
        serde_json::from_value(json!({
            "label":"Read",
            "output":{"body":{"text":{"source":"result","path":"/text"}}}
        }))
        .unwrap(),
    );
    let summary = raw.summary();
    assert!(summary.has_details && summary.diagnostics);
    assert_eq!(summary.bytes, Some(original.to_string().len()));
    let detail = raw.detail();
    assert_eq!(detail.result, Some(original));
    assert_eq!(
        detail.resolved.unwrap().output.unwrap().body.unwrap().text,
        "Literal stream 中文 🙂"
    );
    let content = detail.content.unwrap();
    let Block::Text(text) = &content.blocks[0] else {
        panic!("text expected");
    };
    assert_eq!(
        text.read(detail.result.as_ref().unwrap()),
        Some("Literal stream 中文 🙂")
    );
    assert_eq!(summary.revision, raw.summary().revision);
    raw.format = ToolFormat::Json;
    assert!(!raw.summary().diagnostics);
    assert!(
        raw.detail()
            .resolved
            .unwrap()
            .output
            .unwrap()
            .body
            .is_none()
    );
}

#[test]
fn selects_mcp_progress_by_format() {
    let arguments =
        json!({"title":"Task","steps":[{"description":"Inspect","state":"in_progress"}]});
    let original = json!({"content":[],"structuredContent":{"progress":arguments}});
    let mut raw = source(original.clone());
    raw.arguments = arguments;
    raw.presentation = Presentation::Progress;
    assert!(raw.summary().progress.is_none());
    raw.format = ToolFormat::Mcp;
    assert_eq!(raw.summary().progress.unwrap().steps.len(), 1);
    assert_eq!(raw.detail().result, Some(original));
}

#[test]
fn business_errors_do_not_become_failures() {
    for result in [
        json!({"error":"business value"}),
        json!({"isError":true,"error":"business value"}),
        json!({"output":{"isError":true,"error":"domain row"}}),
    ] {
        let mut raw = source(result.clone());
        raw.presentation = Presentation::Hidden;
        let summary = raw.summary();
        assert!(!summary.diagnostics && !summary.has_details);
        assert!(summary.failure.is_none());
        raw.failure = Some(Failure { code: None });
        let summary = raw.summary();
        assert!(summary.diagnostics && summary.has_details);
        assert_eq!(raw.detail().result, Some(result));
    }
}

#[test]
fn command_outcomes_require_declared_content() {
    use sailry_protocol::process::{Capture, Completion, Outcome};
    for outcome in [
        Outcome::Exited(7),
        Outcome::Signal(9),
        Outcome::TimedOut,
        Outcome::Cancelled,
        Outcome::Unknown("cleanup is unconfirmed".into()),
    ] {
        let mut result = serde_json::to_value(sailry_protocol::Output::CommandResult(Completion {
            outcome: outcome.clone(),
            stdout: Capture {
                text: "Original stdout".into(),
                ..Default::default()
            },
            stderr: Capture::default(),
            elapsed_ms: 1,
        }))
        .unwrap();
        result["sailry_content"] =
            json!({"version":1,"blocks":[{"kind":"text","path":"/data/stdout/text"}]});
        let mut raw = source(result.clone());
        raw.display = Some(
            serde_json::from_value(json!({"label":"Execute","input":{"summary":{
                "path":"/command","format":"command"
            }}}))
            .unwrap(),
        );
        assert_eq!(raw.summary().resolved.unwrap().command, Some(outcome));
        raw.presentation = Presentation::Details;
        assert!(raw.summary().resolved.unwrap().command.is_none());
        assert_eq!(raw.detail().result, Some(result));
    }
}
