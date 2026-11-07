use super::*;
use futures::StreamExt;

#[tokio::test]
async fn retains_incomplete_anthropic_calls() {
    for broken in [false, true] {
        for arguments in ["{\"path\":\"file\"}", "{\"path\":", ""] {
            let body = fixtures::sse([
                json!({
                    "type":"message_start",
                    "message":{"id":"partial","type":"message","role":"assistant",
                        "model":"claude-opus-4-8","content":[],
                        "usage":{"input_tokens":12,"output_tokens":3,
                            "cache_read_input_tokens":8,"cache_creation_input_tokens":4}}
                }),
                json!({"type":"content_block_start","index":0,
                    "content_block":{"type":"tool_use","id":"unfinished","name":"read","input":{}}}),
                json!({"type":"content_block_delta","index":0,
                    "delta":{"type":"input_json_delta","partial_json":arguments}}),
            ]);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len() + if broken { 100 } else { 0 }
            );
            let server = Server::start(vec![reply]).await;
            let provider = provider(ModelApi::Anthropic, server.endpoint.clone());
            let model = native(&provider, "claude-opus-4-8", "key", HeaderMap::new());
            let mut stream = model
                .call(request("claude-opus-4-8"), CancellationToken::new())
                .await
                .unwrap();
            let mut calls = Vec::new();
            let mut finish = None;
            let mut usage = None;
            let mut failure = None;
            while let Some(event) = stream.next().await {
                match event {
                    Ok(Event::ToolCall { call, .. }) => calls.push(call),
                    Ok(Event::Finish(reason)) => finish = Some(reason),
                    Ok(Event::Usage(reported)) => usage = Some(reported),
                    Err(error) => failure = Some(error),
                    _ => {}
                }
            }
            if broken {
                assert_eq!(failure.unwrap().kind, ErrorKind::Network, "{arguments}");
                assert!(calls.is_empty(), "{arguments}");
                assert!(finish.is_none(), "{arguments}");
            } else {
                assert!(failure.is_none(), "{failure:?}");
                assert_eq!(calls.len(), 1, "{arguments}");
                assert_eq!(calls[0].id, "unfinished");
                assert_eq!(calls[0].name, "read");
                assert_eq!(
                    calls[0].error.as_ref().map(|error| error.code),
                    Some(-32602)
                );
                assert!(finish.is_some(), "{arguments}");
            }
            let usage = usage.expect("retained partial usage");
            assert_eq!(usage.input, Some(24));
            assert_eq!(usage.output, Some(3));
            assert_eq!(usage.total, Some(27));
            assert_eq!(usage.cache_read, Some(8));
            assert_eq!(usage.cache_write, Some(4));
            assert_eq!(server.finish().await.len(), 1);
        }
    }
}
