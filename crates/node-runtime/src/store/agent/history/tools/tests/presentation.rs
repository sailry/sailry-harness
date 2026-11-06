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
fn preserves_storage_contracts() {
    for (scenario, check) in [
        (
            "content_requires_canonical_images_and_valid_blocks",
            (|scenario: &'static str| {
                let original = json!({"output":{"sailry_content":{"version":1,"blocks":[
                    {"kind":"table","columns":["Name"],"rows":[["中文 🙂"]]},
                    {"kind":"image","index":2}
                ]}}});
                let mut raw = source(original.clone());
                assert!(raw.detail().content.is_none(), "{scenario}");
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
                assert_eq!(raw.detail().content.unwrap().blocks.len(), 2, "{scenario}");
                assert!(raw.summary().has_details, "{scenario}");
                raw.failure = Some(Failure { code: None });
                assert!(raw.detail().content.is_none(), "{scenario}");
                assert!(raw.summary().diagnostics, "{scenario}");
                raw.failure = None;
                raw.result.as_mut().unwrap()["output"]["sailry_content"]["version"] = json!(2);
                assert!(raw.detail().content.is_none(), "{scenario}");
                assert_eq!(
                    original["output"]["sailry_content"]["version"], 1,
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "publishes_both_artifact_kinds_without_eager_content",
            (|scenario: &'static str| {
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
                        assert!(raw.summary().published, "{scenario}");
                        assert!(
                            !serde_json::to_string(&raw.summary())
                                .unwrap()
                                .contains("Compare approaches"),
                            "{scenario}"
                        );
                        assert_eq!(raw.detail().content, Some(content), "{scenario}");
                        assert_eq!(raw.detail().result, Some(original), "{scenario}");
                    }
                }
            }),
        ),
        (
            "published_files_require_inline_content_for_disclosure",
            (|scenario: &'static str| {
                let file = json!({"kind":"file","path":"index.html","mime":"text/html"});
                let mut raw = source(
                    json!({"sailry_content":{"version":1,"blocks":[file]},"text":"Original HTML"}),
                );
                assert!(raw.summary().published, "{scenario}");
                assert!(!raw.summary().has_details, "{scenario}");
                assert!(raw.detail().content.is_some(), "{scenario}");
                raw.result.as_mut().unwrap()["sailry_content"]["blocks"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"kind":"text","path":"/text"}));
                assert!(raw.summary().published, "{scenario}");
                assert!(raw.summary().has_details, "{scenario}");
                assert_eq!(
                    raw.detail().result.as_ref().unwrap()["text"],
                    "Original HTML",
                    "{scenario}"
                );
            }),
        ),
        (
            "preserves_failed_card_receipts",
            (|scenario: &'static str| {
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
                assert!(raw.summary().published, "{scenario}");
                assert_eq!(
                    raw.detail().content.unwrap().blocks,
                    vec![Block::Card(card)],
                    "{scenario}"
                );
                assert_eq!(raw.detail().result, Some(original), "{scenario}");
            }),
        ),
        (
            "progress_requires_matching_valid_arguments",
            (|scenario: &'static str| {
                let arguments = json!({"title":"Task 中文 🙂","steps":[
                    {"description":"Inspect","state":"completed"},{"description":"Verify","state":"in_progress"}
                ]});
                let mut raw = source(json!({"progress":arguments}));
                raw.arguments = arguments;
                raw.presentation = Presentation::Progress;
                assert_eq!(raw.summary().progress.unwrap().steps.len(), 2, "{scenario}");
                assert!(raw.summary().has_details, "{scenario}");
                raw.result.as_mut().unwrap()["progress"]["steps"][0]["state"] = json!("pending");
                assert!(raw.summary().progress.is_none(), "{scenario}");
                raw.result = Some(json!({"progress":raw.arguments,"error":"not recorded"}));
                assert!(raw.summary().progress.is_none(), "{scenario}");
                raw.presentation = Presentation::Details;
                assert!(raw.summary().progress.is_none(), "{scenario}");
            }),
        ),
        (
            "presents_mcp_content_without_changing_raw_values",
            (|scenario: &'static str| {
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
                assert!(summary.has_details && summary.diagnostics, "{scenario}");
                assert_eq!(
                    summary.bytes,
                    Some(original.to_string().len()),
                    "{scenario}"
                );
                let detail = raw.detail();
                assert_eq!(detail.result, Some(original), "{scenario}");
                assert_eq!(
                    detail.resolved.unwrap().output.unwrap().body.unwrap().text,
                    "Literal stream 中文 🙂",
                    "{scenario}"
                );
                let content = detail.content.unwrap();
                let Block::Text(text) = &content.blocks[0] else {
                    panic!("{scenario}: text expected");
                };
                assert_eq!(
                    text.read(detail.result.as_ref().unwrap()),
                    Some("Literal stream 中文 🙂"),
                    "{scenario}"
                );
                assert_eq!(summary.revision, raw.summary().revision, "{scenario}");
                raw.format = ToolFormat::Json;
                assert!(!raw.summary().diagnostics, "{scenario}");
                assert!(
                    raw.detail()
                        .resolved
                        .unwrap()
                        .output
                        .unwrap()
                        .body
                        .is_none(),
                    "{scenario}"
                );
            }),
        ),
        (
            "summarizes_delegation_background_mode",
            (|scenario: &'static str| {
                let mut raw = Raw::new(
                    Some("spawn".into()),
                    "spawn_agent".into(),
                    json!({"task":"Inspect","background":true}),
                );
                assert!(!raw.summary().delegation.background, "{scenario}");
                raw.delegation = true;
                assert!(raw.summary().delegation.background, "{scenario}");
                for arguments in [
                    json!({"task":"Inspect"}),
                    json!({"task":"Inspect","background":false}),
                    json!({"task":"Inspect","background":"true"}),
                ] {
                    raw.arguments = arguments;
                    assert!(!raw.summary().delegation.background, "{scenario}");
                }
                let command = Raw::new(
                    Some("command".into()),
                    "run_command".into(),
                    json!({"command":"sleep 5","background":true}),
                );
                assert!(!command.summary().delegation.background, "{scenario}");
            }),
        ),
        (
            "selects_mcp_progress_by_format",
            (|scenario: &'static str| {
                let arguments = json!({"title":"Task","steps":[{"description":"Inspect","state":"in_progress"}]});
                let original = json!({"content":[],"structuredContent":{"progress":arguments}});
                let mut raw = source(original.clone());
                raw.arguments = arguments;
                raw.presentation = Presentation::Progress;
                assert!(raw.summary().progress.is_none(), "{scenario}");
                raw.format = ToolFormat::Mcp;
                assert_eq!(raw.summary().progress.unwrap().steps.len(), 1, "{scenario}");
                assert_eq!(raw.detail().result, Some(original), "{scenario}");
            }),
        ),
        (
            "retains_bounded_mcp_failure_messages",
            (|scenario: &'static str| {
                let message = "Original failure 中文 🙂\n".repeat(100);
                for (format, original) in [
                    (
                        ToolFormat::McpError,
                        json!({"code":-32603,"message":message,"data":{"code":"outcome_unknown"}}),
                    ),
                    (
                        ToolFormat::Mcp,
                        json!({"isError":true,"content":[{"type":"text","text":message}]}),
                    ),
                ] {
                    let mut raw = source(original.clone());
                    raw.format = format;
                    raw.presentation = Presentation::Summary;
                    assert!(raw.summary().error.is_none(), "{scenario}");
                    raw.failure = Some(Failure { code: None });
                    let summary = raw.summary();
                    assert!(summary.diagnostics && summary.has_details, "{scenario}");
                    assert_eq!(
                        summary.error.unwrap(),
                        message.chars().take(1024).collect::<String>(),
                        "{scenario}"
                    );
                    assert_eq!(raw.detail().result, Some(original), "{scenario}");
                }
            }),
        ),
        (
            "business_errors_do_not_become_failures",
            (|scenario: &'static str| {
                for (format, result) in [
                    (ToolFormat::Json, json!({"error":"business value"})),
                    (
                        ToolFormat::Json,
                        json!({"isError":true,"error":"business value"}),
                    ),
                    (
                        ToolFormat::Json,
                        json!({"output":{"isError":true,"error":"domain row"}}),
                    ),
                    (
                        ToolFormat::Mcp,
                        json!({"isError":false,"content":[],"structuredContent":{"isError":true,"error":"domain row"}}),
                    ),
                ] {
                    let mut raw = source(result.clone());
                    raw.format = format;
                    raw.presentation = Presentation::Hidden;
                    let summary = raw.summary();
                    assert!(!summary.diagnostics && !summary.has_details, "{scenario}");
                    assert!(summary.failure.is_none(), "{scenario}");
                    assert!(summary.error.is_none(), "{scenario}");
                    raw.failure = Some(Failure { code: None });
                    let summary = raw.summary();
                    assert!(summary.diagnostics && summary.has_details, "{scenario}");
                    assert_eq!(raw.detail().result, Some(result), "{scenario}");
                }
            }),
        ),
        (
            "command_outcomes_require_declared_content",
            (|scenario: &'static str| {
                use sailry_protocol::process::{Capture, Completion, Outcome};
                for outcome in [
                    Outcome::Exited(7),
                    Outcome::Signal(9),
                    Outcome::TimedOut,
                    Outcome::Cancelled,
                    Outcome::Unknown("cleanup is unconfirmed".into()),
                ] {
                    let mut result =
                        serde_json::to_value(sailry_protocol::Output::CommandResult(Completion {
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
                    assert_eq!(
                        raw.summary().resolved.unwrap().command,
                        Some(outcome),
                        "{scenario}"
                    );
                    raw.presentation = Presentation::Details;
                    assert!(
                        raw.summary().resolved.unwrap().command.is_none(),
                        "{scenario}"
                    );
                    assert_eq!(raw.detail().result, Some(result), "{scenario}");
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
