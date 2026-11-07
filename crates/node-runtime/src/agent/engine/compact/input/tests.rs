use super::*;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};

fn result(id: &str, text: &str) -> Message {
    Message::user().with_tool_response(
        id,
        Ok(CallToolResult::success(vec![ContentBlock::text(text)])),
    )
}

fn request(id: &str, text: &str) -> Message {
    Message::assistant().with_tool_request(
        id,
        Ok(CallToolRequestParams::new("read_file").with_arguments(
            serde_json::json!({"path":text})
                .as_object()
                .unwrap()
                .clone(),
        )),
    )
}

fn prepared(messages: &[Message], budget: usize) -> anyhow::Result<String> {
    let overhead = tokens::text(&render(PROMPT, &serde_json::json!({"messages":""})).unwrap())
        + REQUEST_TOKENS;
    Ok(prepare(messages, budget + overhead + 8_000, 8_000)?
        .iter()
        .map(Message::as_concat_text)
        .collect::<Vec<_>>()
        .join("\n"))
}

#[test]
fn preserves_latest_tool_rounds() {
    let old = format!(
        "Old start {} OLD_MIDDLE {} Old end",
        "文🙂".repeat(700),
        "字🙂".repeat(700)
    );
    let latest = format!(
        "Latest start {} LATEST_MIDDLE {} Latest end",
        "x".repeat(2_000),
        "y".repeat(2_000)
    );
    let history = vec![
        Message::user().with_text("First request"),
        request("old-call", "src/old.rs"),
        result("old-call", &old),
        Message::assistant().with_text("Old result reviewed"),
        Message::user().with_text("Current request"),
        request("new-call", "src/new.rs"),
        result("new-call", &latest),
        request("second-call", "src/another.rs"),
        result("second-call", "Second result"),
    ];
    let original = history.clone();
    let text = prepared(&history, 50_000).unwrap();
    assert!(text.contains("Old start"));
    assert!(text.contains("Old end"));
    assert!(!text.contains("OLD_MIDDLE"));
    assert!(text.contains("characters omitted"));
    assert!(text.contains(&latest));
    assert!(text.contains("Second result"));
    assert!(text.contains("src/new.rs"));
    assert_eq!(history, original);
    let input = prepare(&history, 64_000, 8_000).unwrap();
    assert!(
        input
            .iter()
            .flat_map(|message| &message.content)
            .all(|content| matches!(content, Content::Text { .. }))
    );
}

#[test]
fn excludes_reasoning_from_every_turn() {
    let history = vec![
        Message::user().with_text("Earlier goal"),
        Message::assistant().with_thinking("Old private reasoning", "old-signature"),
        Message::user().with_text("Current goal"),
        Message::assistant().with_thinking("Latest private reasoning", "new-signature"),
        Message::assistant().with_text("Actual result"),
    ];
    let text = prepared(&history, 1_000).unwrap();
    assert!(text.contains("Actual result"));
    assert!(!text.contains("reasoning"));
    assert!(!text.contains("signature"));
}

#[test]
fn omits_oldest_turn_only_as_last_resort() {
    let history = vec![
        Message::user().with_text(format!(
            "Old request {}",
            "must preserve this constraint ".repeat(1_000)
        )),
        request("old", &"old_argument ".repeat(1_000)),
        result("old", &"old_result ".repeat(2_000)),
        Message::assistant().with_text("Old assistant explanation ".repeat(1_000)),
        Message::user().with_text("Newest exact request"),
        result("new", "Newest exact result"),
    ];
    let text = prepared(&history, 100).unwrap();
    assert!(text.contains("Newest exact request"));
    assert!(text.contains("Newest exact result"));
    assert!(!text.contains("Old request"));
    assert!(!text.contains("old_result"));
}

#[test]
fn retains_user_words_before_omitting_turns() {
    let history = vec![
        Message::user().with_text("Do not change the public API"),
        result("old", &"Old result ".repeat(2_000)),
        Message::assistant().with_text("Detailed assistant explanation ".repeat(1_000)),
        Message::user().with_text("Continue the implementation"),
    ];
    let text = prepared(&history, 100).unwrap();
    assert!(text.contains("Do not change the public API"));
    assert!(text.contains("Continue the implementation"));
    assert!(!text.contains("Old result"));
    assert!(!text.contains("Detailed assistant"));
}

#[test]
fn rejects_oversized_latest_turn() {
    let history = vec![
        Message::user().with_text("Earlier task"),
        Message::user().with_text("Current task"),
        result("current", &"Keep all latest evidence ".repeat(1_000)),
    ];
    let original = history.clone();
    let error = prepared(&history, 100).unwrap_err();
    assert!(error.to_string().contains("latest turn"));
    assert_eq!(history, original);
}

#[test]
fn counts_prompt_and_generation_reserve() {
    let history = vec![Message::user().with_text("Keep the goal")];
    assert!(prepare(&history, 8_000, 8_000).is_err());
    assert!(prepare(&history, 64_000, 8_000).is_ok());
}

#[test]
fn preserves_unicode_at_both_edges() {
    let text = format!("{}middle{}", "你🙂".repeat(500), "好🦀".repeat(500));
    let reduced = edges(&text);
    assert!(reduced.starts_with(&"你🙂".repeat(500)));
    assert!(reduced.ends_with(&"好🦀".repeat(500)));
    assert!(reduced.contains("[6 characters omitted]"));
    assert_eq!(edges("short"), "short");
}
