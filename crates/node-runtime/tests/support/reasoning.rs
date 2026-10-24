//! Stateless replay fixtures shared by API-key and OAuth service contracts.
use sailry_protocol::SessionId;
use serde_json::{Value, json};

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

pub fn stream(index: usize) -> Vec<u8> {
    let event = json!({"type":"response.completed","sequence_number":0,
        "response":{"id":format!("response-{index}"),"object":"response","created_at":1,
            "model":"fixture-a","status":"completed","output":output(index)}});
    let body = format!("data: {event}\n\ndata: [DONE]\n\n");
    format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
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

pub async fn events(node: &sailry_node_runtime::Node, session: SessionId) -> Vec<Vec<u8>> {
    node.with_storage(move |connection| {
        Box::pin(async move {
            let mut rows = connection
                .query(
                    "SELECT body FROM agent_events WHERE session=?1 ORDER BY rowid",
                    [session.to_string()],
                )
                .await
                .map_err(crate::database_fixture::fault)?;
            let mut bodies = Vec::new();
            while let Some(row) = rows.next().await.map_err(crate::database_fixture::fault)? {
                bodies.push(row.get(0).map_err(crate::database_fixture::fault)?);
            }
            Ok(bodies)
        })
    })
    .await
    .unwrap()
}
