use super::*;
use web_search::{FIRST, SECOND, URI};

pub const ENCRYPTED: &str = "encrypted-search-result-fixture";
pub const INDEX: &str = "encrypted-citation-index-fixture";

#[derive(Clone, Copy)]
pub enum Reply {
    Search,
    Pause,
    SearchError,
    RequestTooLarge,
    Truncated,
}

pub fn blocks(request: &Value, reply: Reply) -> (Vec<Value>, &'static str) {
    let search = request["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool["type"] == "web_search_20250305")
    });
    let resumed = request["messages"]
        .as_array()
        .and_then(|messages| messages.last())
        .is_some_and(|message| message["role"] == "assistant");
    let returned = request["messages"]
        .to_string()
        .contains("Search companion content");
    let tool = !returned
        && request["tools"].as_array().is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| tool["name"] == plugin_tool("files", "read_file"))
        });
    let mut blocks = Vec::new();
    if search && !returned {
        if !resumed {
            blocks.push(
                json!({"type":"server_tool_use", "id":"search-fixture", "name":"web_search",
                "input":{"query":"fixture source 🙂"}}),
            );
            if matches!(reply, Reply::Pause) {
                return (blocks, "pause_turn");
            }
        }
        blocks.push(
            json!({"type":"web_search_tool_result", "tool_use_id":"search-fixture",
            "content": if matches!(reply, Reply::SearchError | Reply::RequestTooLarge) {
                json!({"type":"web_search_tool_result_error", "error_code":
                    if matches!(reply, Reply::RequestTooLarge) { "request_too_large" } else { "unavailable" }})
            } else {
                json!([{"type":"web_search_result", "url":URI, "title":"Fixture source",
                    "encrypted_content":ENCRYPTED}])
            }}),
        );
    }
    for text in [FIRST, SECOND] {
        let mut block = json!({"type":"text", "text":text});
        if search && !matches!(reply, Reply::SearchError | Reply::RequestTooLarge) {
            block["citations"] = json!([{"type":"web_search_result_location", "url":URI,
                "title":"Fixture source", "encrypted_index":INDEX, "cited_text":"Fixture source passage"}]);
        }
        blocks.push(block);
    }
    if tool {
        blocks.push(
            json!({"type":"tool_use", "id":"read-fixture", "name":plugin_tool("files", "read_file"),
            "input":{"path":"native.txt"}}),
        );
    }
    (blocks, if tool { "tool_use" } else { "end_turn" })
}

pub(super) async fn respond(
    mut stream: TcpStream,
    request: &Value,
    reply: Reply,
) -> std::io::Result<()> {
    let (blocks, stop) = blocks(request, reply);
    let mut message = json!({"id":"anthropic-fixture", "type":"message", "role":"assistant",
        "model":request["model"], "content":[], "stop_reason":null, "stop_sequence":null,
        "usage":{"input_tokens":8,"output_tokens":0,"cache_read_input_tokens":3}});
    if request["stream"] != true {
        message["content"] = json!(blocks);
        message["stop_reason"] = json!(stop);
        message["usage"]["output_tokens"] = json!(4);
        let body = message.to_string();
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
        return stream.shutdown().await;
    }
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    event(
        &mut stream,
        json!({"type":"message_start", "message":message}),
    )
    .await?;
    for (index, block) in blocks.into_iter().enumerate() {
        let mut start = block.clone();
        let delta = match block["type"].as_str().unwrap() {
            "text" => {
                start["text"] = json!("");
                start.as_object_mut().unwrap().remove("citations");
                Some((
                    "text_delta",
                    "text",
                    block["text"].as_str().unwrap().to_owned(),
                ))
            }
            "tool_use" | "server_tool_use" => {
                start["input"] = json!({});
                Some((
                    "input_json_delta",
                    "partial_json",
                    block["input"].to_string(),
                ))
            }
            _ => None,
        };
        event(
            &mut stream,
            json!({"type":"content_block_start", "index":index, "content_block":start}),
        )
        .await?;
        if let Some((kind, field, value)) = delta {
            let middle = value
                .char_indices()
                .nth(value.chars().count() / 2)
                .unwrap()
                .0;
            for chunk in [&value[..middle], &value[middle..]] {
                let mut delta = json!({"type":kind});
                delta[field] = json!(chunk);
                event(
                    &mut stream,
                    json!({"type":"content_block_delta", "index":index, "delta":delta}),
                )
                .await?;
            }
        }
        for citation in block["citations"].as_array().into_iter().flatten() {
            event(
                &mut stream,
                json!({"type":"content_block_delta", "index":index,
                "delta":{"type":"citations_delta", "citation":citation}}),
            )
            .await?;
        }
        event(
            &mut stream,
            json!({"type":"content_block_stop", "index":index}),
        )
        .await?;
    }
    event(
        &mut stream,
        json!({"type":"message_delta", "delta":{"stop_reason":stop,"stop_sequence":null},
        "usage":{"output_tokens":4}}),
    )
    .await?;
    if !matches!(reply, Reply::Truncated) {
        event(&mut stream, json!({"type":"message_stop"})).await?;
    }
    stream.shutdown().await
}

async fn event(stream: &mut TcpStream, value: Value) -> std::io::Result<()> {
    stream
        .write_all(
            format!(
                "event: {}\ndata: {value}\n\n",
                value["type"].as_str().unwrap()
            )
            .as_bytes(),
        )
        .await
}
