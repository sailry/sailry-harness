use super::*;
use sailry_protocol::{
    AttachmentId, WorktreeId, attachment, conversation::Image, tool::Presentation,
};

fn fixture() -> Page {
    let mut page = page();
    if let Part::ToolCall {
        presentation,
        arguments,
        ..
    } = &mut page.entries[0].parts[0]
    {
        *presentation = Presentation::Content;
        *arguments = json!({"presentation":"summary"});
    }
    let mut result = entry(2, page.runs[0].turn, "", true);
    result.parts[0] = Part::ToolResult {
        id: Some("call".into()),
        name: "read_file".into(),
        result: json!({"output":{"sailry_content":{"version":1,"blocks":[
            {"kind":"table","columns":["Name"],"rows":[["中文 🙂"]]},
            {"kind":"image","index":2}
        ]}}}),
        images: vec![Image {
            entry: result.id.clone(),
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
        }],
    };
    page.entries.push(result);
    page
}

mod artifacts {
    use super::*;
    use sailry_protocol::{artifact, tool};

    #[test]
    fn restores_both_kinds() {
        for kind in [artifact::Kind::Document, artifact::Kind::Preview] {
            for wrapped in [false, true] {
                let mut page = fixture();
                let artifact = artifact::Summary {
                    id: sailry_protocol::ArtifactId::new(),
                    kind,
                    session: page.session,
                    turn: page.runs[0].turn,
                    package: sailry_protocol::plugin::Reference {
                        name: "document-fixture".into(),
                        digest: "a".repeat(64),
                        settings_revision: 0,
                    },
                    title: "Options".into(),
                    summary: "Compare the approaches".into(),
                    revision: 1,
                    size: 12,
                    created_at: 10,
                    updated_at: 10,
                };
                let content = tool::Content {
                    version: 1,
                    blocks: vec![tool::Block::Artifact { artifact }],
                };
                let native = json!({"sailry_content":content,"original":"retained"});
                let original = if wrapped {
                    json!({"output":native})
                } else {
                    native
                };
                if let Part::ToolResult { result, images, .. } = &mut page.entries[1].parts[0] {
                    *result = original.clone();
                    images.clear();
                }
                let calls = collect(&page);
                assert_eq!(calls[0].state, State::Returned);
                assert_eq!(calls[0].content, Some(content));
                assert_eq!(calls[0].result(&page), Some(&original));
                let restored: Page =
                    serde_json::from_value(serde_json::to_value(&page).unwrap()).unwrap();
                assert_eq!(collect(&restored), calls);
                assert_eq!(calls[0].result(&restored), Some(&original));
            }
        }
    }
}

mod cards {
    use super::*;

    #[test]
    fn restores_failed_cards_without_changing_results() {
        let mut page = fixture();
        let summary = sailry_protocol::plugin::card::Summary {
            receipt: sailry_protocol::plugin::card::Receipt {
                session: page.session,
                turn: page.runs[0].turn,
                call: sailry_protocol::RequestId::new(),
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
        let original = json!({"isError":true,"error":{"code":"not_configured","message":"Connect"},"sailry_content":{"version":1,"blocks":[sailry_protocol::tool::Block::Card(summary.clone())]}});
        if let Part::ToolResult { result, images, .. } = &mut page.entries[1].parts[0] {
            *result = original.clone();
            images.clear();
        }
        page.entries[1]
            .parts
            .push(Part::Resource(json!({"type":"tool_failed",
            "index":0,"id":"call","name":"read_file","failure":{"code":"not_configured"}})));
        let calls = collect(&page);
        assert_eq!(
            calls[0].content.as_ref().unwrap().blocks,
            vec![sailry_protocol::tool::Block::Card(summary)]
        );
        assert_eq!(calls[0].result(&page), Some(&original));
        let restored: Page = serde_json::from_value(serde_json::to_value(&page).unwrap()).unwrap();
        assert_eq!(collect(&restored), calls);
        assert_eq!(calls[0].result(&restored), Some(&original));
        if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
            result["sailry_content"]["blocks"][0]["receipt"]["session"] =
                json!(sailry_protocol::SessionId::new());
        }
        assert!(collect(&page)[0].content.is_none());
    }
}

#[test]
fn restores() {
    let page = fixture();
    let calls = collect(&page);
    assert_eq!(calls[0].content.as_ref().unwrap().blocks.len(), 2);
    let restored: Page = serde_json::from_value(serde_json::to_value(&page).unwrap()).unwrap();
    assert_eq!(collect(&restored), calls);
    assert_eq!(
        serde_json::to_value(&calls).unwrap()[0]["content"]["version"],
        1
    );
    assert!(calls[0].result(&page).unwrap()["output"]["sailry_content"].is_object());
}

#[test]
fn business_fields_keep_declared_content() {
    let mut page = fixture();
    if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
        result["isError"] = json!(false);
        result["output"]["isError"] = json!(true);
        result["output"]["error"] = json!("domain row");
    }
    let original = page.clone();
    let calls = collect(&page);
    assert!(calls[0].failure.is_none());
    assert!(calls[0].content.is_some());
    assert!(calls[0].details(&page));
    assert!(!calls[0].diagnostics(&page));
    assert_eq!(page, original);
    page.entries[1]
        .parts
        .push(Part::Resource(json!({"type":"tool_failed",
        "index":0,"id":"call","name":"read_file","failure":{}})));
    let failed = collect(&page);
    assert!(failed[0].failure.is_some());
    assert!(failed[0].content.is_none());
    assert!(failed[0].diagnostics(&page));
    assert!(failed[0].details(&page));
}

#[test]
fn association() {
    let original = fixture();
    for change in 0..9 {
        let mut page = original.clone();
        match change {
            0 => {
                page.entries.remove(0);
            }
            1 => page.entries[1].turn = TurnId::new(),
            2 => page.entries[1].branch = "other".into(),
            3 => {
                if let Part::ToolCall { presentation, .. } = &mut page.entries[0].parts[0] {
                    *presentation = Presentation::Details;
                }
            }
            4 => {
                if let Part::ToolResult { name, .. } = &mut page.entries[1].parts[0] {
                    *name = "different".into();
                }
            }
            5 => {
                if let Part::ToolResult { images, .. } = &mut page.entries[1].parts[0] {
                    images.clear();
                }
            }
            6 => {
                if let Part::ToolResult { images, .. } = &mut page.entries[1].parts[0] {
                    images[0].index = 1;
                }
            }
            7 => {
                if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
                    result["error"] = json!("failed");
                }
                page.entries[1]
                    .parts
                    .push(Part::Resource(json!({"type":"tool_failed",
                    "index":0,"id":"call","name":"read_file","failure":{}})));
            }
            8 => {
                if let Part::ToolResult { result, .. } = &mut page.entries[1].parts[0] {
                    result["output"]["sailry_content"]["version"] = json!(2);
                }
            }
            _ => unreachable!(),
        }
        let calls = collect(&page);
        assert!(
            calls.iter().all(|call| call.content.is_none()),
            "change {change}"
        );
        assert!(calls.iter().any(|call| call.result(&page).is_some()));
    }
}
