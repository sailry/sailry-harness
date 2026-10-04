use super::*;

pub const COMMENTARY: &str = "**Creating JSON fixture**\n\n**";
pub const ANSWER: &str = "The file was read";

pub(super) async fn respond(
    mut stream: TcpStream,
    request: &Value,
    tool: Option<(String, Value)>,
) -> std::io::Result<()> {
    let returned = request["input"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["type"] == "function_call_output")
    });
    let (phase, text) = if returned {
        ("final_answer", ANSWER)
    } else {
        ("commentary", COMMENTARY)
    };
    let message = json!({"type":"message", "id":"phase-message", "role":"assistant",
        "status":"completed", "phase":phase,
        "content":[{"type":"output_text", "text":text, "annotations":[]}]});
    let mut output = vec![message.clone()];
    if !returned {
        let (name, arguments) = tool.unwrap_or_else(|| {
            (
                plugin_tool("files", "read_file"),
                json!({"path":"phase.txt"}),
            )
        });
        output.push(
            json!({"type":"function_call", "id":"read-item", "call_id":"read-phase",
            "name":name, "arguments":arguments.to_string(), "status":"completed"}),
        );
    }
    let response = json!({"id":"phase-response", "object":"response", "created_at":1,
        "status":"completed", "model":request["model"], "output":output});
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    let mut added = message;
    added["content"] = json!([]);
    added["status"] = json!("in_progress");
    let events = [
        json!({"type":"response.output_item.added", "sequence_number":0, "output_index":0, "item":added}),
        json!({"type":"response.output_text.delta", "sequence_number":1,
            "item_id":"phase-message", "output_index":0, "content_index":0, "delta":text, "logprobs":[]}),
        json!({"type":"response.completed", "sequence_number":2, "response":response}),
    ];
    for event in events {
        stream
            .write_all(format!("data: {event}\n\n").as_bytes())
            .await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stream.write_all(b"data: [DONE]\n\n").await?;
    stream.shutdown().await
}
