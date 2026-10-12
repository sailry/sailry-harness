use super::*;
use crate::anthropic::{AnthropicClient, AnthropicConfig};
use crate::retry::RetryConfig;
use adk_core::{Content, GenerateContentConfig, Llm, LlmRequest, Part};
use futures::TryStreamExt;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

fn client(server: &MockServer) -> AnthropicClient {
    let mut headers = HeaderMap::new();
    headers.insert("x-fixture", HeaderValue::from_static("original"));
    headers.insert(
        "anthropic-version",
        HeaderValue::from_static("fixture-version"),
    );
    headers.insert(
        "anthropic-beta",
        HeaderValue::from_static("fixture-beta,structured-outputs-2025-11-13"),
    );
    AnthropicClient::new(
        AnthropicConfig::new("fixture-key", "claude-opus-5")
            .with_base_url(format!("{}/gateway", server.uri()))
            .with_fast_mode(true)
            .with_context_management(adk_anthropic::ContextManagement::clear_thinking()),
    )
    .unwrap()
    .with_default_headers(headers)
    .with_retry_config(RetryConfig::disabled())
}

fn request() -> LlmRequest {
    let mut request = LlmRequest::new(
        "claude-opus-5",
        vec![
            Content::new("system").with_text("System instructions"),
            Content::new("user").with_text("Hello"),
        ],
    );
    let mut config = GenerateContentConfig::default();
    config.extensions.insert(
        "anthropic".into(),
        json!({"built_in_tools":[{
            "type":"custom", "name":"fixture", "strict":true,
            "input_schema":{"type":"object","properties":{},"additionalProperties":false}
        }]}),
    );
    request.config = Some(config);
    request
}

fn response(streaming: bool) -> ResponseTemplate {
    let message = json!({
        "id":"msg_fixture", "type":"message", "role":"assistant",
        "model":"claude-opus-5", "content":[{"type":"text","text":"done"}],
        "stop_reason":"end_turn", "stop_sequence":null,
        "usage":{"input_tokens":10,"output_tokens":2,"cache_read_input_tokens":4}
    });
    if !streaming {
        return ResponseTemplate::new(200).set_body_json(message);
    }
    let mut start = message;
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    start["usage"]["output_tokens"] = json!(0);
    let events = [
        json!({"type":"message_start","message":start}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ];
    let body = events
        .iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<String>();
    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
}

async fn mount(server: &MockServer, streaming: bool, count: u64) {
    Mock::given(method("POST"))
        .and(path("/gateway/v1/messages"))
        .respond_with(response(streaming))
        .expect(count)
        .mount(server)
        .await;
}

#[tokio::test]
async fn replaces_parameters_and_authentication() {
    for streaming in [false, true] {
        let server = MockServer::start().await;
        mount(&server, streaming, 1).await;
        let adapter: RequestAdapter = Arc::new(move |body, headers| {
            assert_eq!(body["stream"], streaming);
            assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
            assert_eq!(body["tools"][0]["strict"], true);
            assert_eq!(
                body["context_management"]["edits"][0]["type"],
                "clear_thinking_20251015"
            );
            body["metadata"] = json!({"user_id":"fixture-user"});
            body["max_tokens"] = json!(321);
            body["system"][0]["text"] = json!("Adapted instructions");
            body["tools"][0]["description"] = json!("Adapted tool");
            headers.remove("x-api-key");
            headers.remove("x-fixture");
            headers.insert(
                header::AUTHORIZATION,
                HeaderValue::from_static("Bearer fixture-token"),
            );
            Ok(())
        });
        let replies = client(&server)
            .with_request_adapter(adapter)
            .generate_content(request(), streaming)
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
        let final_reply = replies.last().unwrap();
        assert!(final_reply.turn_complete);
        let usage = final_reply.usage_metadata.as_ref().unwrap();
        assert_eq!(usage.prompt_token_count, 14);
        assert_eq!(usage.cache_read_input_token_count, Some(4));
        assert!(
            final_reply
                .content
                .as_ref()
                .unwrap()
                .parts
                .iter()
                .any(|part| { matches!(part, Part::Text { text } if text == "done") })
        );
        let received = server.received_requests().await.unwrap();
        assert_eq!(received.len(), 1);
        let headers = &received[0].headers;
        assert!(!headers.contains_key("x-api-key"));
        assert!(!headers.contains_key("x-fixture"));
        assert_eq!(headers[header::AUTHORIZATION], "Bearer fixture-token");
        assert_eq!(headers["anthropic-version"], "fixture-version");
        assert_eq!(
            headers[header::ACCEPT],
            if streaming {
                "text/event-stream"
            } else {
                "application/json"
            }
        );
        assert_eq!(
            headers["anthropic-beta"],
            "fixture-beta,structured-outputs-2025-11-13,context-management-2025-06-27,fast-mode-2026-02-01"
        );
        let body: Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["metadata"]["user_id"], "fixture-user");
        assert_eq!(body["max_tokens"], 321);
        assert_eq!(body["system"][0]["text"], "Adapted instructions");
        assert_eq!(body["tools"][0]["description"], "Adapted tool");
        assert_eq!(body["cache_control"]["type"], "ephemeral");
        assert_eq!(body["stream"], streaming);
    }
}

#[tokio::test]
async fn preserves_default_requests() {
    for streaming in [false, true] {
        let server = MockServer::start().await;
        mount(&server, streaming, 2).await;
        let unchanged: RequestAdapter = Arc::new(|_, _| Ok(()));
        for adapter in [None, Some(unchanged)] {
            let mut client = client(&server);
            if let Some(adapter) = adapter {
                client = client.with_request_adapter(adapter);
            }
            client
                .generate_content(request(), streaming)
                .await
                .unwrap()
                .try_collect::<Vec<_>>()
                .await
                .unwrap();
        }
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].body, requests[1].body);
        assert_eq!(requests[0].headers, requests[1].headers);
    }
}

#[tokio::test]
async fn propagates_adapter_errors_before_dispatch() {
    for streaming in [false, true] {
        let server = MockServer::start().await;
        let adapter: RequestAdapter = Arc::new(|_, _| {
            Err(AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::InvalidInput,
                "fixture.adapter",
                "Request adaptation failed",
            ))
        });
        let error = client(&server)
            .with_request_adapter(adapter)
            .generate_content(request(), streaming)
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap_err();
        assert_eq!(error.code, "fixture.adapter");
        assert_eq!(error.message, "Request adaptation failed");
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn rejects_invalid_parameters_before_dispatch() {
    for streaming in [false, true] {
        let server = MockServer::start().await;
        let adapter: RequestAdapter = Arc::new(|body, _| {
            body["messages"] = json!("invalid");
            Ok(())
        });
        let error = client(&server)
            .with_request_adapter(adapter)
            .generate_content(request(), streaming)
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap_err();
        assert_eq!(error.code, "model.anthropic.request_adapter");
        assert_eq!(error.category, ErrorCategory::InvalidInput);
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn rejects_response_mode_changes() {
    for streaming in [false, true] {
        let server = MockServer::start().await;
        let adapter: RequestAdapter = Arc::new(move |body, _| {
            body["stream"] = json!(!streaming);
            Ok(())
        });
        let error = client(&server)
            .with_request_adapter(adapter)
            .generate_content(request(), streaming)
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap_err();
        assert_eq!(error.code, "model.anthropic.request_adapter");
        assert_eq!(error.category, ErrorCategory::InvalidInput);
        assert_eq!(
            error.message,
            "Request adapter changed Anthropic response mode"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn preserves_disabled_retries() {
    for streaming in [false, true] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503).set_body_json(
                json!({"type":"error","error":{"type":"overloaded_error","message":"Unavailable"}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let observed = attempts.clone();
        let adapter: RequestAdapter = Arc::new(move |_, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        let client = client(&server).with_request_adapter(adapter);
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            client
                .generate_content(request(), streaming)
                .await?
                .try_collect::<Vec<_>>()
                .await
        })
        .await
        .expect("Disabled retries must return the first failure");
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn allows_beta_header_removal() {
    let server = MockServer::start().await;
    let client = client(&server);
    let params = AnthropicClient::build_message_params(
        "claude-opus-5",
        4096,
        &request(),
        client.anthropic_config(),
    )
    .unwrap();
    let adapter: RequestAdapter = Arc::new(|_, headers| {
        headers.remove("anthropic-beta");
        Ok(())
    });
    let (_, headers) = prepare(client.inner(), params, &adapter).unwrap();
    assert!(!headers.contains_key("anthropic-beta"));
}

#[tokio::test]
async fn dropping_unpolled_stream_skips_dispatch() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let adapter: RequestAdapter = Arc::new(move |_, _| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let client = client(&server).with_request_adapter(adapter);
    let stream = client.generate_content(request(), true).await.unwrap();
    drop(stream);
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
    assert!(server.received_requests().await.unwrap().is_empty());
}
