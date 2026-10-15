use serde_json::{Value, json};

const FORMATTED: &str = "Title: Public source\nURL: https://example.com/source\nPublished: 2026-10-09\nAuthor: N/A\nHighlights:\nUntrusted content\n\n---\n\nTitle: Other source\nURL: https://example.com/other\nPublished: N/A\nAuthor: N/A\nText: More content";

pub fn response(method: &str, body: &Value, modern: bool) -> (u16, String) {
    if method == "DELETE" {
        (204, String::new())
    } else if method == "GET" {
        (405, String::new())
    } else if body["method"] == "server/discover" {
        let result = if modern {
            let version =
                body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"].clone();
            Ok(json!({
                "resultType":"complete", "supportedVersions":[version],
                "capabilities":{"tools":{}}, "ttlMs":0, "cacheScope":"private"
            }))
        } else {
            Err(json!({"code":-32601,"message":"Method not found"}))
        };
        let response = match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":body["id"],"result":result}),
            Err(error) => json!({"jsonrpc":"2.0","id":body["id"],"error":error}),
        };
        (200, response.to_string())
    } else if body.get("id").is_none() {
        (202, String::new())
    } else {
        let result = if body["method"] == "initialize" {
            json!({"protocolVersion":body["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"Search fixture","version":"1"}})
        } else {
            let mut result = json!({"content":[{"type":"text","text":FORMATTED}],"isError":false});
            if modern {
                result["resultType"] = json!("complete");
            }
            result
        };
        (
            200,
            json!({"jsonrpc":"2.0","id":body["id"],"result":result}).to_string(),
        )
    }
}
