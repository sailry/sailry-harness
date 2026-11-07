use super::*;
use futures::StreamExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use std::sync::Arc;

#[expect(
    dead_code,
    reason = "The shared HTTP fixture includes modes used by integration tests"
)]
#[path = "../../../../../tests/discovery_support/mod.rs"]
mod discovery;
#[expect(
    dead_code,
    reason = "The shared provider fixture includes modes used by integration tests"
)]
#[path = "../../../../../tests/agent/providers/native/server.rs"]
mod native_server;
#[path = "../../../../../tests/agent_support/provider/key.rs"]
mod provider_fixture;

#[tokio::test]
async fn never_restarts_a_partial_stream() {
    let server = discovery::Server::start_with_request(ModelApi::Anthropic, |_| {
        let body = concat!(
            "data: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"fixture-response\",\"object\":\"response\",\"created_at\":0,\"model\":\"fixture\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"fixture-item\",\"output_index\":0,\"content_index\":0,\"delta\":\"partial\",\"logprobs\":[]}\n\n",
            "data: invalid-json\n\n",
        );
        discovery::Reply::Raw(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()))
    }).await;
    let provider = provider(ModelApi::Responses, server.endpoint.clone());
    let model = native(&provider, "fixture", "isolated-key", HeaderMap::new());
    let request = Request {
        model: "fixture".into(),
        messages: vec![Message::user().with_text("Isolated request")],
        ..Request::default()
    };
    let mut stream = crate::agent::model::retry::stream(
        Arc::new(model),
        request,
        crate::agent::model::retry::Policy {
            retries: 5,
            delay: Duration::ZERO,
            buffered: false,
        },
        CancellationToken::new(),
        CancellationToken::new(),
    );
    let mut partial = String::new();
    let mut failed = false;
    while let Some(response) = stream.next().await {
        match response {
            Ok(Event::TextDelta { text, .. }) => partial.push_str(&text),
            Ok(_) => {}
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    assert_eq!(partial, "partial");
    assert!(failed);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn preserves_parallel_image_results() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let server = native_server::Server::start(api, native_server::Reply::Text).await;
        let provider = provider(api, server.endpoint.clone());
        let model = native(&provider, "fixture-a", native_server::KEY, HeaderMap::new());
        let calls = Message::assistant()
            .with_tool_request("capture", Ok(CallToolRequestParams::new("capture")))
            .with_tool_request("read", Ok(CallToolRequestParams::new("read")));
        let response = |id: &str, image: bool| {
            let mut result = CallToolResult::structured(json!({"ok":true}));
            if image {
                result
                    .content
                    .push(ContentBlock::image("AQID", "image/png"));
            }
            Message::user().with_tool_response(id, Ok(result))
        };
        let contents = vec![
            Message::user().with_text("Inspect the tool images"),
            calls,
            response("capture", true),
            response("read", false),
            Message::assistant()
                .with_thinking("Both tools completed", "")
                .with_text("Observed"),
            Message::user().with_text("Continue"),
        ];
        let canonical = contents.clone();
        collect(
            &model,
            Request {
                model: "fixture-a".into(),
                messages: contents.clone(),
                ..Request::default()
            },
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(contents, canonical);
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let body = &requests[0].body;
        assert!(body.get("tools").is_none());
        assert!(!body.to_string().contains("sailry_history"));
        assert!(!body.to_string().contains("__sailry_history_tools"));
        if api == ModelApi::ChatCompletions {
            let messages = body["messages"].as_array().unwrap();
            let outputs: Vec<_> = messages
                .iter()
                .filter(|message| message["role"] == "tool")
                .collect();
            assert_eq!(outputs.len(), 2);
            assert_eq!(outputs[0]["tool_call_id"], "capture");
            assert_eq!(outputs[1]["tool_call_id"], "read");
            assert!(
                outputs
                    .iter()
                    .all(|output| output["content"].as_str().unwrap().contains("true"))
            );
            assert!(messages.iter().any(|message| {
                message["role"] == "user"
                    && message["content"]
                        .to_string()
                        .contains("data:image/png;base64,AQID")
            }));
            let observed = messages
                .iter()
                .find(|message| message["content"] == "Observed")
                .unwrap();
            assert_eq!(observed["reasoning_content"], "Both tools completed");
            assert_eq!(messages.last().unwrap()["content"], "Continue");
        } else {
            let outputs: Vec<_> = body["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "function_call_output")
                .collect();
            assert_eq!(outputs.len(), 2);
            assert_eq!(outputs[0]["call_id"], "capture");
            assert_eq!(outputs[1]["call_id"], "read");
            assert!(
                outputs[0]["output"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|part| part["type"] == "input_image"
                        && part["image_url"] == "data:image/png;base64,AQID")
            );
            assert!(!outputs[1]["output"].to_string().contains("input_image"));
        }
    }
}

#[tokio::test]
#[ignore = "requires explicit permission and incurs one real model request"]
async fn probes_one_request_without_tools() {
    const MODEL: &str = "deepseek-v4.1-flash";
    let (provider, secret) = provider_fixture::load(MODEL);
    assert_eq!(provider.api, ModelApi::OpenCodeGo);
    assert_eq!(
        provider.endpoint.trim_end_matches('/'),
        "https://opencode.ai/zen/go/v1"
    );
    let model = native(&provider, MODEL, secret.expose(), HeaderMap::new());
    let request = Request {
        model: MODEL.into(),
        messages: vec![Message::user().with_text("Reply with OK")],
        max_tokens: Some(128),
        ..Request::default()
    };
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        model
            .call(request, CancellationToken::new())
            .await?
            .try_collect::<Vec<_>>()
            .await
    })
    .await;
    match result {
        Ok(Ok(events)) => assert!(!text(&events).is_empty()),
        Ok(Err(_)) => panic!("model request failed"),
        Err(_) => panic!("model request timed out"),
    }
}
