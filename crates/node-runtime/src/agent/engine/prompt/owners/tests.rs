use super::*;
use crate::agent::canonical::{Block, Compaction, Message as StoredMessage, Role, ToolFormat};
use rmcp::model::CallToolResult;
use sailry_protocol::TurnId;
use serde_json::Value;

fn stored(name: &str, id: &str, value: Value) -> Record {
    let response = ToolResponse {
        id: id.into(),
        name: name.into(),
        status: ToolStatus::Success,
        result: value.clone().into(),
        revision: blake3::hash(value.to_string().as_bytes())
            .to_hex()
            .to_string(),
        attachments: Vec::new(),
        provider: None,
        format: ToolFormat::Json,
        meta: None,
    };
    Record::new(
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
    )
}

fn source(name: &str, id: &str, directory: &str, content: &str) -> (Record, Message) {
    let value = json!({
        "skill":"fixture:analysis","path":"SKILL.md","content":content,
        "directory":directory,"python":"/fixture/python",
    });
    let record = stored(name, id, value.clone());
    let mut message = Message::user().with_tool_response(id, Ok(CallToolResult::structured(value)));
    message.id = Some(record.id.clone());
    (record, message)
}

fn result(message: &Message) -> &CallToolResult {
    let Content::ToolResult(response) = &message.content[0] else {
        panic!("tool response expected")
    };
    response.result.as_ref().unwrap()
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
    let projected = messages.clone();
    owners.project(&mut messages);
    assert_eq!(messages, projected);
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
    let Content::ToolResult(response) = &mut message.content[0] else {
        panic!("tool response expected")
    };
    response
        .result
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

#[test]
fn bounds_skills_across_compactions() {
    let sized = |name: &str, repeat: usize| {
        stored(
            "load_skill",
            name,
            json!({
                "skill":name,"path":"SKILL.md","content":"Skill body ".repeat(repeat),
                "directory":format!("/package/{name}"),
            }),
        )
    };
    let skill = |name: &str| sized(name, 800);
    let reload = |name: &str| {
        format!(
            "{PREFIX}skill:{name}:SKILL.md\nOmitted to bound context. Call load_skill with skill {name} and path SKILL.md again if it is still needed."
        )
    };
    let split = |parts: Vec<(String, String)>| {
        let (omitted, full): (Vec<_>, Vec<_>) = parts
            .into_iter()
            .partition(|(_, part)| part.contains("\nOmitted to bound context."));
        (full, omitted)
    };
    let keys =
        |parts: &[(String, String)]| parts.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>();
    let compaction = || {
        let now = chrono::Utc::now();
        Record::new(
            TurnId::new(),
            Kind::Compaction(Compaction {
                boundary: "fixture-boundary".into(),
                summary: StoredMessage::default(),
                start: now,
                end: now,
                files: Vec::new(),
            }),
        )
    };
    let mut records = vec![stored(
        "update_plan",
        "plan",
        json!({"progress":{"title":"Plan","steps":[]}}),
    )];
    for index in 0..20 {
        records.push(skill(&format!("fixture:skill-{index}")));
        records.push(compaction());
    }
    records.push(skill("fixture:skill-0"));
    records.push(compaction());
    records.push(skill("fixture:current"));
    let (full, omitted) = split(collect(&records).parts());
    // The reloaded skill is the most recent; older ones exceed the budget.
    assert_eq!(
        keys(&full),
        [
            "plan",
            "skill:fixture:current:SKILL.md",
            "skill:fixture:skill-0:SKILL.md",
            "skill:fixture:skill-19:SKILL.md",
        ]
    );
    let earlier: usize = full
        .iter()
        .filter(|(key, _)| key.starts_with("skill:fixture:skill-"))
        .map(|(_, part)| part.chars().count())
        .sum();
    assert!(earlier <= SKILL_CHARS, "{earlier}");
    let mut expected: Vec<_> = (1..19)
        .map(|index| format!("fixture:skill-{index}"))
        .map(|name| (format!("skill:{name}:SKILL.md"), reload(&name)))
        .collect();
    expected.sort();
    assert_eq!(omitted, expected);

    // An oversized most recent skill stays; the older one becomes a reload
    // line that a later compaction leaves unchanged.
    let mut records = vec![
        stored(
            "update_plan",
            "plan",
            json!({"progress":{"title":"Plan","steps":[]}}),
        ),
        skill("fixture:older"),
        compaction(),
        sized("fixture:oversized", 3_000),
        compaction(),
    ];
    let (full, omitted) = split(collect(&records).parts());
    assert_eq!(keys(&full), ["plan", "skill:fixture:oversized:SKILL.md"]);
    assert!(full[1].1.chars().count() > SKILL_CHARS);
    let line = vec![(
        "skill:fixture:older:SKILL.md".to_owned(),
        reload("fixture:older"),
    )];
    assert_eq!(omitted, line);
    records.push(compaction());
    let (full, omitted) = split(collect(&records).parts());
    assert_eq!(keys(&full), ["plan", "skill:fixture:oversized:SKILL.md"]);
    assert_eq!(omitted, line);
}
