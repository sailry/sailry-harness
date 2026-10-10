use super::*;

#[derive(Clone)]
pub(super) struct Reply {
    pub(super) gates: Arc<Vec<Arc<tokio::sync::Notify>>>,
    pub(super) content_type: Option<String>,
    pub(super) ending: &'static str,
}

pub(super) async fn respond(
    mut stream: TcpStream,
    body: &Value,
    reply: Reply,
) -> std::io::Result<()> {
    assert_eq!(body["stream"], true);
    assert_eq!(reply.gates.len(), 3);
    let content_type = reply
        .content_type
        .as_ref()
        .map_or(String::new(), |value| format!("Content-Type: {value}\r\n"));
    stream
        .write_all(format!("HTTP/1.1 200 OK\r\n{content_type}Connection: close\r\n\r\n").as_bytes())
        .await?;
    for (index, text) in ["First", " second", " third"].iter().enumerate() {
        let event = json!({"type":"response.output_text.delta", "sequence_number":index,
            "item_id":"stream-answer", "output_index":0, "content_index":0,
            "delta":text, "logprobs":[]});
        write(&mut stream, &event, reply.ending).await?;
        reply.gates[index].notified().await;
    }
    let event = json!({"type":"response.completed", "sequence_number":3,
        "response":{"id":"stream-response", "object":"response", "created_at":1,
            "model":body["model"], "status":"completed", "usage":{"input_tokens":8,"input_tokens_details":{"cached_tokens":0},
                "output_tokens":3,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":11},
            "output":[{"type":"message", "id":"stream-answer", "role":"assistant", "status":"completed",
                "content":[{"type":"output_text", "text":"First second third", "annotations":[]}]}]}});
    write(&mut stream, &event, reply.ending).await?;
    stream.shutdown().await
}

async fn write(stream: &mut TcpStream, event: &Value, ending: &str) -> std::io::Result<()> {
    // Every byte boundary also splits CRLF pairs and multibyte JSON when present.
    let encoded = format!("data: {event}{ending}{ending}");
    for byte in encoded.as_bytes() {
        stream.write_all(std::slice::from_ref(byte)).await?;
    }
    Ok(())
}
