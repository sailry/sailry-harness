//! Stateless replay fixtures shared by API-key and OAuth service contracts.
use sailry_protocol::SessionId;
use serde_json::{Value, json};
use std::path::Path;

pub fn output(index: usize) -> Value {
    let mut reasoning = json!({"type":"reasoning","id":format!("stateless-{index}"),"status":"completed","summary":[]});
    if index == 0 {
        reasoning["summary"] =
            json!([{"type":"summary_text","text":"Legacy summary without encrypted content"}]);
    } else {
        reasoning["encrypted_content"] = json!(format!("sealed {index} +/ opaque"));
    }
    json!([reasoning, {"type":"message","id":format!("answer-{index}"),"role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":format!("Stateless answer {index}"),"annotations":[]}]}])
}

pub fn payload(body: &Value, index: usize) {
    assert_eq!(body["store"], false);
    assert!(body.get("previous_response_id").is_none());
    assert!(
        body["include"]
            .as_array()
            .unwrap()
            .contains(&json!("reasoning.encrypted_content"))
    );
    assert!(body["reasoning"].get("summary").is_none());
    let reasoning: Vec<_> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "reasoning")
        .collect();
    assert_eq!(reasoning.len(), index.saturating_sub(1));
    for (item, previous) in reasoning.into_iter().zip(1..index) {
        assert_eq!(item["id"], format!("stateless-{previous}"));
        assert_eq!(
            item["encrypted_content"],
            format!("sealed {previous} +/ opaque")
        );
        assert_eq!(item["summary"], json!([]));
    }
    assert!(!body.to_string().contains("stateless-0"));
}

pub fn events(profile: &Path, session: SessionId) -> Vec<Vec<u8>> {
    let database = rusqlite::Connection::open_with_flags(
        profile.join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    database
        .prepare("SELECT body FROM agent_events WHERE session=?1 ORDER BY rowid")
        .unwrap()
        .query_map([session.to_string()], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
