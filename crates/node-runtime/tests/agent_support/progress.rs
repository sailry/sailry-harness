use super::*;

pub const PROGRESS: &str = "**Creating JSON fixture**\n\n**";
pub const ANSWER: &str = "The file was read";

pub(super) async fn respond(
    stream: TcpStream,
    request: &Value,
    tool: Option<(String, Value)>,
) -> std::io::Result<()> {
    reply(stream, request, tool, false).await
}

pub(super) async fn respond_empty(stream: TcpStream, request: &Value) -> std::io::Result<()> {
    reply(stream, request, None, true).await
}

async fn reply(
    mut stream: TcpStream,
    request: &Value,
    tool: Option<(String, Value)>,
    empty: bool,
) -> std::io::Result<()> {
    let returned = request["input"].as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["type"] == "function_call_output")
    });
    let text = if returned { ANSWER } else { PROGRESS };
    let response_id = format!("progress-{}", uuid::Uuid::new_v4());
    let message_id = format!("message-{response_id}");
    let message = json!({"type":"message", "id":message_id, "role":"assistant",
        "status":"completed",
        "content":[{"type":"output_text", "text":text, "annotations":[]}]});
    let mut output = if empty && returned {
        vec![]
    } else {
        vec![message.clone()]
    };
    if !returned {
        let (name, arguments) = tool.unwrap_or_else(|| {
            (
                plugin_tool("files", "read_file"),
                json!({"path":"progress.txt"}),
            )
        });
        output.push(
            json!({"type":"function_call", "id":"read-item", "call_id":"read-progress",
            "name":offered(request, &name), "arguments":arguments.to_string(), "status":"completed"}),
        );
    }
    let response = json!({"id":response_id, "object":"response", "created_at":1,
        "status":"completed", "model":request["model"], "output":output});
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    let mut added = message;
    added["content"] = json!([]);
    added["status"] = json!("in_progress");
    let created = json!({"type":"response.created", "sequence_number":0,
        "response":{"id":response_id, "object":"response", "created_at":1,
            "status":"in_progress", "model":request["model"], "output":[]}});
    let mut events = vec![
        json!({"type":"response.output_item.added", "sequence_number":1, "output_index":0, "item":added}),
        json!({"type":"response.output_text.delta", "sequence_number":2,
            "item_id":message_id, "output_index":0, "content_index":0, "delta":text, "logprobs":[]}),
        json!({"type":"response.completed", "sequence_number":3, "response":response}),
    ];
    if empty && returned {
        events.drain(..2);
    }
    events.insert(0, created);
    for event in events {
        stream
            .write_all(format!("data: {event}\n\n").as_bytes())
            .await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    stream.write_all(b"data: [DONE]\n\n").await?;
    stream.shutdown().await
}
