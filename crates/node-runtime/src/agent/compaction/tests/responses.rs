//! Exercise the pinned Responses HTTP/SSE decoder, not fabricated LlmResponse parts.
use super::*;
use adk_model::openai::{OpenAIResponsesClient, OpenAIResponsesConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn model(events: Vec<serde_json::Value>) -> (Arc<dyn Llm>, tokio::task::JoinHandle<()>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
            if let Some(header) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= header + 4 + length {
                    break;
                }
            }
        }
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        for event in events {
            stream
                .write_all(format!("data: {event}\n\n").as_bytes())
                .await
                .unwrap();
        }
        stream.write_all(b"data: [DONE]\n\n").await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let config = OpenAIResponsesConfig::new("fixture", "fixture").with_base_url(endpoint);
    (
        Arc::new(
            OpenAIResponsesClient::new(config)
                .unwrap()
                .with_response_recovery(true),
        ),
        server,
    )
}

fn message(id: &str, phase: &str, text: &str) -> serde_json::Value {
    json!({"type":"message", "id":id, "role":"assistant", "status":"completed", "phase":phase,
        "content":[{"type":"output_text", "text":text, "annotations":[]}]})
}

fn completed(output: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"type":"response.completed", "response":{"id":"summary-response", "object":"response",
        "created_at":1, "status":"completed", "model":"fixture", "output":output,
        "usage":{"input_tokens":100, "output_tokens":20, "total_tokens":120}}})
}

#[tokio::test]
async fn commentary_precedes_final_summary() {
    for mode in [SummaryMode::Manual, SummaryMode::Automatic] {
        let commentary = message("commentary", "commentary", "Preparing handoff");
        let mut added = commentary.clone();
        added["content"] = json!([]);
        added["status"] = json!("in_progress");
        let events = vec![
            json!({"type":"response.output_item.added", "sequence_number":0, "output_index":0, "item":added}),
            json!({"type":"response.output_text.delta", "sequence_number":1, "item_id":"commentary",
                "output_index":0, "content_index":0, "delta":"Preparing handoff", "logprobs":[]}),
            completed(vec![
                commentary,
                message(
                    "final",
                    "final_answer",
                    "Verified source; continue remaining work",
                ),
            ]),
        ];
        let (model, server) = model(events).await;
        let (mut summarizer, _) = summarizer(vec![], false);
        summarizer.model = model;
        let event = summarizer
            .summarize_with(&history(), mode)
            .await
            .unwrap()
            .unwrap();
        let text = event.actions.compaction.unwrap().compacted_content.parts;
        let serialized = serde_json::to_string(&text).unwrap();
        assert!(serialized.contains("Verified source; continue remaining work"));
        assert!(!serialized.contains("Preparing handoff"));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn rejects_tools_and_truncated_output() {
    let tool = completed(vec![
        json!({"type":"function_call", "id":"call-item", "call_id":"call-1",
        "name":"read_file", "arguments":"{}", "status":"completed"}),
    ]);
    let mut truncated = completed(vec![message("final", "final_answer", "Unfinished summary")]);
    truncated["type"] = json!("response.incomplete");
    truncated["response"]["status"] = json!("incomplete");
    truncated["response"]["incomplete_details"] = json!({"reason":"max_output_tokens"});
    let mut interrupted = truncated.clone();
    interrupted["response"]["incomplete_details"] = json!({"reason":"interrupted"});
    for event in [tool, truncated, interrupted] {
        let (model, server) = model(vec![event]).await;
        let (mut summarizer, _) = summarizer(vec![], false);
        summarizer.model = model;
        assert!(
            summarizer
                .summarize_with(&history(), SummaryMode::Manual)
                .await
                .is_err()
        );
        server.await.unwrap();
    }
}
