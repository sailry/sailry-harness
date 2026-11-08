use super::*;
use crate::agent::model::{
    Content as Part, Message, Model,
    testing::{collect, roundtrip, server::Server},
};
use rmcp::model::{CallToolResult, ContentBlock as Content, Tool};
use sailry_protocol::conversation::connection::Options;
use serde_json::{Value, json};

const ANTHROPIC_API_VERSION: &str = "2023-06-01";

mod configuration;
mod google;

fn provider(api: ModelApi, endpoint: String) -> Provider {
    serde_json::from_value(json!({
        "id":sailry_protocol::ProviderId::new(),"revision":1,"name":"Captured provider",
        "api":api,"authentication":"api_key","endpoint":endpoint,"enabled":true,
        "models":[],"default_model":"gpt-6.1-sol","credential":null
    }))
    .unwrap()
}

fn tool() -> Tool {
    Tool::new(
        "read",
        "Read a file",
        serde_json::from_value::<serde_json::Map<String, Value>>(
            json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        )
        .unwrap(),
    )
}

fn response(tool: bool) -> String {
    let output = if tool {
        json!([{"type":"function_call","id":"item","status":"completed","call_id":"stable-call","name":"read","arguments":"{\"path\":\"file\"}"}])
    } else {
        json!([{"type":"message","id":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Read complete","annotations":[]}]}])
    };
    let created = json!({"type":"response.created","sequence_number":0,"response":{"id":"response","object":"response","created_at":1,"model":"gpt-6.1-sol","status":"in_progress","output":[]}});
    let done = json!({"type":"response.completed","sequence_number":1,"response":{"id":"response","object":"response","created_at":1,"model":"gpt-6.1-sol","status":"completed","output":output,"usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15,"input_tokens_details":{"cached_tokens":8}}}});
    format!("data: {created}\n\ndata: {done}\n\ndata: [DONE]\n\n")
}

#[tokio::test]
async fn responses_use_sse_and_stable_calls() {
    let server = Server::start(vec![response(true), response(false)]).await;
    let registry = registry::Registry::default();
    let session = SessionId::new();
    let provider = provider(
        ModelApi::Responses,
        format!("{}/gateway/v1", server.endpoint),
    );
    let native = native(
        &registry,
        &provider,
        "gpt-6.1-sol",
        session,
        "captured-key",
        HeaderMap::new(),
    )
    .unwrap();
    let config = Config {
        model_name: "gpt-6.1-sol".into(),
        context_limit: Some(100_000),
        max_tokens: Some(512),
        thinking_effort: Effort::High,
        reasoning: Some(true),
        ..Default::default()
    };
    let user = Message::user().with_text("Read the file");
    let (call, usage) = collect(
        native
            .call(
                config.request(
                    "Captured instructions".into(),
                    vec![user.clone()],
                    vec![tool().into()],
                ),
                CancellationToken::new(),
            )
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let request = call
        .content
        .iter()
        .find_map(|block| match block {
            Part::ToolCall(call) => Some(call),
            _ => None,
        })
        .unwrap();
    assert_eq!(request.id, "stable-call");
    assert_eq!(request.arguments["path"], "file");
    assert_eq!(usage.cache_read, Some(8));
    let result = Message::user().with_tool_response(
        request.id.clone(),
        Ok(CallToolResult::success(vec![Content::text(
            "File contents",
        )])),
    );
    let _ = collect(
        native
            .call(
                config.request(
                    "Captured instructions".into(),
                    vec![user, call, result],
                    vec![tool().into()],
                ),
                CancellationToken::new(),
            )
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let requests = server.finish().await;
    for request in &requests {
        assert_eq!(request.path, "/gateway/v1/responses");
        assert_eq!(request.headers["authorization"], "Bearer captured-key");
        assert_eq!(request.headers["content-type"], "application/json");
        assert_eq!(request.body["stream"], true);
        assert_eq!(request.body["prompt_cache_key"], session.to_string());
        assert!(request.body.get("tool_choice").is_none());
        assert!(request.body.get("previous_response_id").is_none());
        assert_eq!(
            request.body["include"],
            json!(["reasoning.encrypted_content"])
        );
        assert!(
            request.body["input"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["type"] != "reasoning")
        );
    }
    let input = requests[1].body["input"].as_array().unwrap();
    assert!(
        input
            .iter()
            .any(|item| item["type"] == "function_call" && item["call_id"] == "stable-call")
    );
    assert!(
        input
            .iter()
            .any(|item| item["type"] == "function_call_output" && item["call_id"] == "stable-call")
    );
}

#[tokio::test]
async fn google_keeps_signed_tool_metadata() {
    let event = json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":"signed-call","name":"read","args":{"path":"file"}},"thoughtSignature":"c2lnbmVkLXRob3VnaHQ="}]},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":3,"totalTokenCount":13},"modelVersion":"gemini-3.5-pro"});
    let server = Server::start(vec![
        format!("data: {event}\n\n"),
        format!("data: {event}\n\n"),
    ])
    .await;
    let registry = registry::Registry::default();
    let provider = provider(
        ModelApi::Gemini,
        format!("{}/proxy/v1beta", server.endpoint),
    );
    let native = native(
        &registry,
        &provider,
        "gemini-3.5-pro",
        SessionId::new(),
        "google-key",
        HeaderMap::new(),
    )
    .unwrap();
    let config = Config {
        model_name: "gemini-3.5-pro".into(),
        ..Default::default()
    };
    let (mut call, _) = collect(
        native
            .call(
                config.request(
                    String::new(),
                    vec![Message::user().with_text("Read")],
                    vec![tool().into()],
                ),
                CancellationToken::new(),
            )
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let request = call
        .content
        .iter_mut()
        .find_map(|block| match block {
            Part::ToolCall(request) => Some(request),
            _ => None,
        })
        .unwrap();
    assert_eq!(request.id, "signed-call");
    assert_eq!(
        request.metadata.as_ref().unwrap()["thoughtSignature"],
        "c2lnbmVkLXRob3VnaHQ="
    );
    request.tool_meta = Some(json!({"title":"Retained metadata"}));
    let call = roundtrip(&call).await;
    let request = call
        .content
        .iter()
        .find_map(|block| match block {
            Part::ToolCall(call) => Some(call),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        request.tool_meta.as_ref().unwrap()["title"],
        "Retained metadata"
    );
    let result = Message::user().with_tool_response(
        "signed-call",
        Ok(CallToolResult::success(vec![Content::text("Done")])),
    );
    let _ = collect(
        native
            .call(
                config.request(String::new(), vec![call, result], vec![tool().into()]),
                CancellationToken::new(),
            )
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let requests = server.finish().await;
    assert_eq!(
        requests[0].path,
        "/proxy/v1beta/models/gemini-3.5-pro:streamGenerateContent?alt=sse"
    );
    assert_eq!(requests[0].headers["x-goog-api-key"], "google-key");
    assert!(
        requests[1].body["contents"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|message| message["parts"].as_array().unwrap())
            .any(|part| part["functionCall"]["name"] == "read"
                && part["thoughtSignature"] == "c2lnbmVkLXRob3VnaHQ=")
    );
}

#[tokio::test]
async fn anthropic_keeps_signed_and_redacted_thinking() {
    let events = [
        json!({"type":"message_start","message":{"id":"message","type":"message","role":"assistant","model":"claude-sonnet-4-6","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Consider"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed-thinking"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"redacted-data"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"call","name":"read","input":{}}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"file\"}"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":4}}),
        json!({"type":"message_stop"}),
    ];
    let body = events
        .into_iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect::<String>();
    for path in ["/v1", "/v1/messages", "/proxy/v1", "/proxy/v1/messages"] {
        let server = Server::start(vec![body.clone(), body.clone()]).await;
        let registry = registry::Registry::default();
        let provider = provider(ModelApi::Anthropic, format!("{}{path}", server.endpoint));
        let native = native(
            &registry,
            &provider,
            "claude-sonnet-4-6",
            SessionId::new(),
            "anthropic-key",
            HeaderMap::new(),
        )
        .unwrap();
        let config = Config {
            model_name: "claude-sonnet-4-6".into(),
            max_tokens: Some(4096),
            thinking_effort: Effort::High,
            ..Default::default()
        };
        let (message, _) = collect(
            native
                .call(
                    config.request(
                        "System".into(),
                        vec![Message::user().with_text("Read")],
                        vec![tool().into()],
                    ),
                    CancellationToken::new(),
                )
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert!(message.content.iter().any(|block|matches!(block,Part::Thinking { signature, .. } if signature.as_deref()==Some("signed-thinking"))));
        assert!(
            message.content.iter().any(
                |block| matches!(block,Part::RedactedThinking { data } if data=="redacted-data")
            )
        );
        let result = Message::user().with_tool_response(
            "call",
            Ok(CallToolResult::success(vec![Content::text("Done")])),
        );
        let _ = collect(
            native
                .call(
                    config.request("System".into(), vec![message, result], vec![tool().into()]),
                    CancellationToken::new(),
                )
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        let requests = server.finish().await;
        assert_eq!(
            requests[0].path,
            if path.starts_with("/proxy") {
                "/proxy/v1/messages"
            } else {
                "/v1/messages"
            }
        );
        assert_eq!(requests[0].headers["x-api-key"], "anthropic-key");
        assert_eq!(
            requests[0].headers["anthropic-version"],
            ANTHROPIC_API_VERSION
        );
        let replay = requests[1].body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|message| message["content"].as_array().unwrap())
            .collect::<Vec<_>>();
        assert!(replay.iter().any(|block|block["type"]=="thinking" && block["signature"]=="signed-thinking"));
        assert!(
            replay
                .iter()
                .any(|block| block["type"] == "redacted_thinking"
                    && block["data"] == "redacted-data")
        );
    }
}

#[tokio::test]
async fn azure_uses_captured_deployment_and_version() {
    let event = json!({"id":"chunk","object":"chat.completion.chunk","created":1,"model":"deployment","choices":[{"index":0,"delta":{"content":"Done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}});
    let server = Server::start(vec![format!("data: {event}\n\ndata: [DONE]\n\n")]).await;
    let registry = registry::Registry::default();
    let mut provider = provider(ModelApi::AzureOpenAi, server.endpoint.clone());
    provider.options = Some(Options::AzureOpenAi {
        api_version: "2025-01-01-preview".into(),
    });
    let native = native(
        &registry,
        &provider,
        "deployment",
        SessionId::new(),
        "azure-key",
        HeaderMap::new(),
    )
    .unwrap();
    let _ = collect(
        native
            .call(
                crate::agent::model::Request {
                    model: "deployment".into(),
                    messages: vec![Message::user().with_text("Hello")],
                    ..Default::default()
                },
                CancellationToken::new(),
            )
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let requests = server.finish().await;
    assert_eq!(
        requests[0].path,
        "/openai/deployments/deployment/chat/completions?api-version=2025-01-01-preview"
    );
    assert_eq!(requests[0].headers["api-key"], "azure-key");
    assert!(!requests[0].headers.contains_key("authorization"));
}

#[tokio::test]
async fn shared_clients_keep_credentials_separate() {
    let server = Server::start(vec![response(false), response(false)]).await;
    let registry = registry::Registry::default();
    let provider = provider(ModelApi::Responses, server.endpoint.clone());
    let first = native(
        &registry,
        &provider,
        "model",
        SessionId::new(),
        "first-captured",
        HeaderMap::new(),
    )
    .unwrap();
    let second = native(
        &registry,
        &provider,
        "model",
        SessionId::new(),
        "second-captured",
        HeaderMap::new(),
    )
    .unwrap();
    let mut request = crate::agent::model::Request {
        model: "model".into(),
        messages: vec![Message::user().with_text("Hello")],
        ..Default::default()
    };
    request
        .headers
        .insert("authorization", "untrusted-override".parse().unwrap());
    request
        .headers
        .insert("x-policy", "captured".parse().unwrap());
    let send = async |provider: Arc<dyn Model>| {
        collect(
            provider
                .call(request.clone(), CancellationToken::new())
                .await
                .unwrap(),
        )
        .await
        .unwrap()
    };
    let _ = tokio::join!(send(first), send(second));
    let requests = server.finish().await;
    let keys: std::collections::BTreeSet<_> = requests
        .iter()
        .map(|request| request.headers["authorization"].as_str())
        .collect();
    assert_eq!(
        keys,
        ["Bearer first-captured", "Bearer second-captured"].into()
    );
    assert!(
        requests
            .iter()
            .all(|request| request.headers["x-policy"] == "captured")
    );
}

#[tokio::test]
async fn retries_rate_limits_before_output() {
    let rejected = "HTTP/1.1 429 Rate limited\r\nContent-Type: application/json\r\nRetry-After: 0\r\nContent-Length: 28\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"Wait\"}}";
    let server = Server::start(vec![rejected.into(), response(false)]).await;
    let registry = registry::Registry::default();
    let provider = provider(ModelApi::Responses, server.endpoint.clone());
    let native = super::native(
        &registry,
        &provider,
        "model",
        SessionId::new(),
        "captured",
        HeaderMap::new(),
    )
    .unwrap();
    let request = crate::agent::model::Request {
        model: "model".into(),
        messages: vec![Message::user().with_text("Hello")],
        ..Default::default()
    };
    let events: Vec<_> = futures::TryStreamExt::try_collect(crate::agent::model::retry::stream(
        native,
        request,
        crate::agent::model::retry::Policy {
            retries: 1,
            delay: std::time::Duration::ZERO,
            buffered: false,
        },
        CancellationToken::new(),
        CancellationToken::new(),
    ))
    .await
    .unwrap();
    assert!(events.iter().any(|event| matches!(event, crate::agent::model::Event::TextDelta { text, .. } if text == "Read complete")));
    assert_eq!(server.finish().await.len(), 2);
}

#[test]
fn maps_captured_effort_and_output() {
    let owner = SessionId::new();
    let mut provider = provider(ModelApi::Responses, "https://example.test/v1".into());
    provider.models = serde_json::from_value(json!([{
        "id":"captured","context":100_000,"output":4096,"vision":true,"reasoning":true,"default_effort":"low"
    }])).unwrap();
    let mut session: SessionConfig = serde_json::from_value(json!({
        "provider":provider.id,"model":"captured","effort":"max","mode":"code","permission":"ask","credential":null
    })).unwrap();
    let frozen = config(&provider, &session, owner);
    assert_eq!(frozen.model_name, "captured");
    assert_eq!(frozen.max_tokens, Some(4096));
    assert_eq!(frozen.context_limit, Some(100_000));
    assert_eq!(frozen.thinking_effort, Effort::Max);
    session.effort = Effort::Low;
    let next = config(&provider, &session, owner);
    assert_eq!(next.thinking_effort, Effort::Low);
    assert_eq!(frozen.thinking_effort, Effort::Max);
    session.effort = Effort::Disabled;
    assert_eq!(
        config(&provider, &session, owner).thinking_effort,
        Effort::Disabled
    );
    assert_eq!(frozen.request_params["prompt_cache_key"], owner.to_string());
    provider.binding = Some(Box::new(sailry_protocol::conversation::presets::Binding {
        policies: vec![sailry_protocol::conversation::presets::RequestPolicy {
            models: vec![session.model.clone()],
            reasoning_replay: None,
            body: json!({"prompt_cache_key":"captured-policy"})
                .as_object()
                .unwrap()
                .clone(),
        }],
        ..Default::default()
    }));
    assert_eq!(
        config(&provider, &session, owner).request_params["prompt_cache_key"],
        "captured-policy"
    );
}

#[tokio::test]
async fn keeps_explicit_chat_routes() {
    for api in [ModelApi::ChatCompletions, ModelApi::DeepSeek] {
        let event = json!({"id":"chat","object":"chat.completion.chunk","created":1,"model":"gpt-6.1-sol","choices":[{"index":0,"delta":{"content":"Done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}});
        let server = Server::start(vec![format!("data: {event}\n\ndata: [DONE]\n\n")]).await;
        let registry = registry::Registry::default();
        let provider = provider(api, format!("{}/v1", server.endpoint));
        let native = native(
            &registry,
            &provider,
            "gpt-6.1-sol",
            SessionId::new(),
            "captured-key",
            HeaderMap::new(),
        )
        .unwrap();
        let config = Config {
            model_name: "gpt-6.1-sol".into(),
            max_tokens: Some(8192),
            thinking_effort: Effort::High,
            reasoning: Some(true),
            ..Default::default()
        };
        let _ = collect(
            native
                .call(
                    config.request(
                        String::new(),
                        vec![Message::user().with_text("Hello")],
                        vec![],
                    ),
                    CancellationToken::new(),
                )
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        let requests = server.finish().await;
        let request = &requests[0];
        assert_eq!(request.path, "/v1/chat/completions");
        assert_eq!(request.body["model"], "gpt-6.1-sol");
        assert_eq!(request.headers["authorization"], "Bearer captured-key");
        assert_eq!(
            request.body[if api == ModelApi::DeepSeek {
                "max_tokens"
            } else {
                "max_completion_tokens"
            }],
            8192
        );
    }
}
