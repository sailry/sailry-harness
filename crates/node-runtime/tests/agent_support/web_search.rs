use super::*;

pub const FIRST: &str = "来源结果 🙂";
pub const SECOND: &str = "Another sourced result";
pub const URI: &str = "https://example.com/source_(one)";

pub(super) async fn respond(
    mut stream: TcpStream,
    request: &Value,
    finish: Option<Arc<tokio::sync::Notify>>,
) -> std::io::Result<()> {
    let mut output = Vec::new();
    let search = request["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|tool| tool["type"] == "web_search"));
    let returned = request["input"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["type"] == "function_call_output")
    });
    if search && !returned {
        output.push(
            json!({"type":"web_search_call", "id":"search-fixture", "status":"completed",
            "action":{"type":"search", "query":"fixture source"}}),
        );
    }
    output.push(json!({"type":"message", "id":"message-fixture", "role":"assistant", "status":"completed",
        "content":([FIRST, SECOND].into_iter().map(|text| json!({"type":"output_text", "text":text,
            "annotations":if search { vec![json!({"type":"url_citation", "start_index":0,
                "end_index":text.chars().count(), "title":"Fixture source", "url":URI})] } else { vec![] }
        })).collect::<Vec<_>>()) }));
    if !returned
        && request["tools"].as_array().is_some_and(|tools| {
            tools.iter().any(|tool| {
                tool["type"] == "function" && tool["name"] == plugin_tool("files", "read_file")
            })
        })
    {
        output.push(json!({"type":"function_call", "id":"read-item", "call_id":"read-fixture",
            "name":plugin_tool("files", "read_file"), "arguments":json!({"path":"native.txt"}).to_string(), "status":"completed"}));
    }
    let response = json!({"id":"response-fixture", "object":"response", "created_at":1,
        "status":"completed", "model":request["model"], "output":output});
    if request["stream"] == true {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await?;
        for (index, text) in [FIRST, SECOND].into_iter().enumerate() {
            let event = json!({"type":"response.output_text.delta", "sequence_number":index,
                "item_id":"message-fixture", "output_index":0, "content_index":index,
                "delta":text, "logprobs":[]});
            stream
                .write_all(format!("data: {event}\n\n").as_bytes())
                .await?;
        }
        if let Some(finish) = finish {
            finish.notified().await;
        }
        let event = json!({"type":"response.completed", "sequence_number":2, "response":response});
        stream
            .write_all(format!("data: {event}\n\ndata: [DONE]\n\n").as_bytes())
            .await?;
    } else {
        let body = response.to_string();
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
    }
    stream.shutdown().await
}
