use serde_json::{Value, json};

const FORMATTED: &str = "Title: Public source\nURL: https://example.com/source\nPublished: 2026-10-09\nAuthor: N/A\nHighlights:\nUntrusted content\n\n---\n\nTitle: Other source\nURL: https://example.com/other\nPublished: N/A\nAuthor: N/A\nText: More content";

pub fn response(method: &str, body: &Value) -> (u16, String) {
    if method == "GET" {
        (405, String::new())
    } else if body.get("id").is_none() {
        (202, String::new())
    } else {
        let result = if body["method"] == "initialize" {
            json!({"protocolVersion":body["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"Search fixture","version":"1"}})
        } else {
            json!({"content":[{"type":"text","text":FORMATTED}],"isError":false})
        };
        (
            200,
            json!({"jsonrpc":"2.0","id":body["id"],"result":result}).to_string(),
        )
    }
}
