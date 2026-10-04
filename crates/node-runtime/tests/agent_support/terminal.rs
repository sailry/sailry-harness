//! A completed response whose HTTP connection deliberately remains open.
use super::*;

pub(super) async fn respond(
    mut stream: TcpStream,
    request: &Value,
    responses: bool,
) -> std::io::Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    let text = "{\"move\":1}";
    let mut events = if responses {
        vec![
            json!({"type":"response.output_text.delta", "sequence_number":0, "item_id":"message", "output_index":0, "content_index":0, "delta":text, "logprobs":[]}),
            json!({"type":"response.completed", "sequence_number":1, "response":{
                "id":"response", "object":"response", "created_at":1, "status":"completed", "model":request["model"],
                "output":[{"type":"message", "id":"message", "role":"assistant", "status":"completed", "content":[{"type":"output_text", "text":text, "annotations":[]}]}],
                "usage":{"input_tokens":12,"output_tokens":4,"total_tokens":16,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}
            }}),
        ]
    } else {
        // Usage arrives separately from finish_reason, as on OpenAI-compatible APIs.
        vec![
            json!({"id":"completion","object":"chat.completion.chunk","created":1,"model":request["model"],"choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
            json!({"id":"completion","object":"chat.completion.chunk","created":1,"model":request["model"],"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
            json!({"id":"completion","object":"chat.completion.chunk","created":1,"model":request["model"],"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":4,"total_tokens":16}}),
        ]
    };
    if request.get("thinking").is_some() {
        // DeepSeek includes usage on the terminal choice instead of a later event.
        let usage = events.pop().unwrap()["usage"].clone();
        events.last_mut().unwrap()["usage"] = usage;
    }
    let incomplete = request.to_string().contains("Truncate the response");
    for mut event in events {
        if incomplete && event["type"] == "response.completed" {
            event["type"] = json!("response.incomplete");
            event["response"]["status"] = json!("incomplete");
            event["response"]["incomplete_details"] = json!({"reason":"max_output_tokens"});
        }
        stream
            .write_all(format!("data: {event}\n\n").as_bytes())
            .await?;
    }
    // No [DONE] or EOF: the caller must stop at the terminal model event.
    let mut byte = [0];
    let _ = stream.read(&mut byte).await?;
    Ok(())
}
