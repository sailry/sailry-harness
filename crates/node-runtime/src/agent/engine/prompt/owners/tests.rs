use super::*;
use crate::agent::canonical::{Block, Kind, Message as StoredMessage, Role, ToolFormat};
use rmcp::model::CallToolResult;
use sailry_protocol::TurnId;
use serde_json::Value;

fn source(name: &str, id: &str, directory: &str, content: &str) -> (Record, Message) {
    let value = json!({
        "skill":"fixture:analysis","path":"SKILL.md","content":content,
        "directory":directory,"python":"/fixture/python",
    });
    let response = ToolResponse {
        id: Some(id.into()),
        name: name.into(),
        status: ToolStatus::Success,
        result: value.clone().into(),
        revision: blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string(),
        attachments: Vec::new(),
        annotations: None,
        provider: None,
        format: ToolFormat::Json,
        meta: None,
    };
    let record = Record::new(
        TurnId::new(),
        Kind::Message(StoredMessage {
            role: Role::User,
            part_count: 1,
            content: vec![Block {
                index: 0,
                kind: BlockKind::ToolResponse(response),
            }],
            ..Default::default()
        }),
    );
    let mut message = Message::user().with_tool_response(id, Ok(CallToolResult::structured(value)));
    message.id = Some(record.id.clone());
    (record, message)
}

fn result(message: &Message) -> &CallToolResult {
    let MessageContent::ToolResponse(response) = &message.content[0] else {
        panic!("tool response expected")
    };
    response.tool_result.as_ref().unwrap()
}

#[test]
fn deduplicates_matching_receipts() {
    let (first, first_message) = source(
        "load_skill",
        "load",
        "/package/digest/analysis",
        "Exact full skill",
    );
    let (second, second_message) = source(
        "read_skill_resource",
        "read",
        "/package/digest/analysis",
        "Exact full skill",
    );
    let records = [first, second];
    let original = serde_json::to_value(&records).unwrap();
    let owners = collect(&records);
    let parts = owners.parts();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].1.matches("Exact full skill").count(), 1);
    let mut messages = [first_message, second_message];
    owners.project(&mut messages);
    for (message, record) in messages.iter().zip(&records) {
        let result = result(message);
        assert!(result.structured_content.is_none());
        assert_eq!(result.content.len(), 1);
        let ContentBlock::Text(text) = &result.content[0] else {
            panic!("JSON text expected")
        };
        let value: Value = serde_json::from_str(&text.text).unwrap();
        assert!(value.get("content").is_none());
        assert_eq!(value["directory"], "/package/digest/analysis");
        assert_eq!(value["python"], "/fixture/python");
        let BlockKind::ToolResponse(source) = &record.message().unwrap().content[0].kind else {
            panic!("canonical response expected")
        };
        assert_eq!(
            value["output"],
            json!({"call":source.id,"revision":source.revision})
        );
        assert!(!text.text.contains("Exact full skill"));
    }
    let projected = serde_json::to_value(&messages).unwrap();
    owners.project(&mut messages);
    assert_eq!(serde_json::to_value(&messages).unwrap(), projected);
    assert_eq!(serde_json::to_value(&records).unwrap(), original);
}

#[test]
fn preserves_other_versions_and_failed_results() {
    let (old, old_message) = source(
        "load_skill",
        "old",
        "/package/old/analysis",
        "Earlier skill",
    );
    let (current, current_message) = source(
        "load_skill",
        "current",
        "/package/current/analysis",
        "Current skill",
    );
    let (mut failed, failed_message) = source(
        "load_skill",
        "failed",
        "/package/current/analysis",
        "Failed skill",
    );
    let BlockKind::ToolResponse(response) = &mut failed.message_mut().unwrap().content[0].kind
    else {
        panic!("canonical response expected")
    };
    response.status = ToolStatus::Error;
    let records = [old, current, failed];
    let owners = collect(&records);
    let parts = owners.parts();
    assert_eq!(parts.len(), 1);
    assert!(parts[0].1.contains("Current skill"));
    assert!(!parts[0].1.contains("Earlier skill"));
    assert!(!parts[0].1.contains("Failed skill"));
    let mut messages = [old_message, current_message, failed_message];
    owners.project(&mut messages);
    assert_eq!(
        result(&messages[0]).structured_content.as_ref().unwrap()["content"],
        "Earlier skill"
    );
    assert!(result(&messages[1]).structured_content.is_none());
    let ContentBlock::Text(text) = &result(&messages[1]).content[0] else {
        panic!("JSON text expected")
    };
    assert!(
        serde_json::from_str::<Value>(&text.text)
            .unwrap()
            .get("content")
            .is_none()
    );
    assert_eq!(
        result(&messages[2]).structured_content.as_ref().unwrap()["content"],
        "Failed skill"
    );
}

#[test]
fn requires_captured_directory_and_exact_result() {
    let (record, message) = source("load_skill", "same", "", "Exact full skill");
    let records = [record];
    let mut messages = [message];
    collect(&records).project(&mut messages);
    assert_eq!(
        result(&messages[0]).structured_content.as_ref().unwrap()["content"],
        "Exact full skill"
    );
    let (record, mut message) = source(
        "load_skill",
        "same",
        "/package/digest/analysis",
        "Exact full skill",
    );
    let MessageContent::ToolResponse(response) = &mut message.content[0] else {
        panic!("tool response expected")
    };
    response
        .tool_result
        .as_mut()
        .unwrap()
        .structured_content
        .as_mut()
        .unwrap()["content"] = json!("Different result");
    let records = [record];
    let mut messages = [message];
    collect(&records).project(&mut messages);
    assert_eq!(
        result(&messages[0]).structured_content.as_ref().unwrap()["content"],
        "Different result"
    );
}
