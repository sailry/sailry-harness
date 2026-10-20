#![cfg(feature = "openai")]

use adk_core::{Content, FinishReason, Llm, LlmRequest, LlmResponse, Part};
use adk_model::openai_compatible::{OpenAICompatible, OpenAICompatibleConfig};
use futures::StreamExt;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn collect(chunks: &[Value]) -> Vec<LlmResponse> {
    let server = MockServer::start().await;
    let body = chunks.iter().map(|chunk| format!("data: {chunk}\n\n")).collect::<String>()
        + "data: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(body)
                .insert_header("content-type", "text/event-stream"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let config = OpenAICompatibleConfig::new("test-key", "gpt-4o")
        .with_provider_name("test")
        .with_base_url(server.uri());
    let client = OpenAICompatible::new(config).expect("client should be created");
    let request = LlmRequest::new("gpt-4o", vec![Content::new("user").with_text("Hello")]);
    client
        .generate_content(request, true)
        .await
        .expect("stream should start")
        .map(|item| item.expect("stream should not fail"))
        .collect()
        .await
}

#[tokio::test]
async fn stop_preserves_terminal_reasoning_and_text() {
    let responses = collect(&[json!({
        "id": "chatcmpl-stop",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {
                "reasoning_content": "I can answer.",
                "content": "The answer is 42."
            },
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 8, "total_tokens": 13}
    })])
    .await;

    assert_eq!(responses.len(), 2);
    let thinking = &responses[0];
    assert!(thinking.partial);
    assert!(!thinking.turn_complete);
    assert_eq!(thinking.finish_reason, None);
    assert!(matches!(
        &thinking.content.as_ref().expect("thinking content").parts[..],
        [Part::Thinking { thinking, .. }] if thinking == "I can answer."
    ));

    let final_response = &responses[1];
    assert!(!final_response.partial);
    assert!(final_response.turn_complete);
    assert_eq!(final_response.finish_reason, Some(FinishReason::Stop));
    assert!(matches!(
        &final_response.content.as_ref().expect("text content").parts[..],
        [Part::Text { text }] if text == "The answer is 42."
    ));
    assert_eq!(
        final_response.usage_metadata.as_ref().expect("terminal usage").total_token_count,
        13
    );
}

#[tokio::test]
async fn tool_calls_preserve_terminal_reasoning_and_arguments() {
    let responses = collect(&[
        json!({
            "id": "chatcmpl-tools",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {"tool_calls": [{
                    "index": 0,
                    "id": "call_weather",
                    "type": "function",
                    "function": {"name": "weather", "arguments": "{\"city\":"}
                }]},
                "finish_reason": null
            }]
        }),
        json!({
            "id": "chatcmpl-tools",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "reasoning_content": "Check the weather.",
                    "tool_calls": [{
                        "index": 0,
                        "function": {"arguments": "\"Paris\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": null
        }),
        json!({
            "id": "chatcmpl-tools",
            "object": "chat.completion.chunk",
            "choices": [],
            "usage": {"prompt_tokens": 8, "completion_tokens": 7, "total_tokens": 15}
        }),
    ])
    .await;

    assert_eq!(responses.len(), 2);
    let thinking = &responses[0];
    assert!(thinking.partial);
    assert!(!thinking.turn_complete);
    assert!(matches!(
        &thinking.content.as_ref().expect("thinking content").parts[..],
        [Part::Thinking { thinking, .. }] if thinking == "Check the weather."
    ));

    let final_response = &responses[1];
    assert!(!final_response.partial);
    assert!(!final_response.turn_complete);
    assert_eq!(final_response.finish_reason, Some(FinishReason::Stop));
    let [Part::FunctionCall { name, args, id, .. }] =
        &final_response.content.as_ref().expect("tool content").parts[..]
    else {
        panic!("expected one function call");
    };
    assert_eq!(name, "weather");
    assert_eq!(args["city"], "Paris");
    assert_eq!(id.as_deref(), Some("call_weather"));
    assert_eq!(
        final_response.usage_metadata.as_ref().expect("terminal usage").total_token_count,
        15
    );
}
