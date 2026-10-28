use super::*;
use goose_provider_types::{
    base::collect_stream,
    conversation::message::{Message, MessageContentBlock},
};
use rmcp::model::{CallToolResult, Content, Tool};
use serde_json::{Value, json};

mod server;
use server::Server;

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
        format!("{}/gateway/v1?route=captured", server.endpoint),
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
    let mut config = ModelConfig::new("gpt-6.1-sol")
        .with_context_limit(Some(100_000))
        .with_max_tokens(Some(512))
        .with_thinking_effort(ThinkingEffort::High);
    config.reasoning = Some(true);
    let user = Message::user().with_text("Read the file");
    let (call, usage) = collect_stream(
        native
            .stream(&config, "Captured instructions", &[user.clone()], &[tool()])
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let request = call
        .content
        .iter()
        .find_map(|block| block.as_tool_request())
        .unwrap();
    assert_eq!(request.id, "stable-call");
    assert_eq!(
        request
            .tool_call
            .as_ref()
            .unwrap()
            .arguments
            .as_ref()
            .unwrap()["path"],
        "file"
    );
    assert_eq!(usage.usage.cache_read_input_tokens, Some(8));
    let result = Message::user().with_tool_response(
        request.id.clone(),
        Ok(CallToolResult::success(vec![Content::text(
            "File contents",
        )])),
    );
    let _ = collect_stream(
        native
            .stream(
                &config,
                "Captured instructions",
                &[user, call, result],
                &[tool()],
            )
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let requests = server.finish().await;
    for request in &requests {
        assert_eq!(request.path, "/gateway/v1/responses?route=captured");
        assert_eq!(request.headers["authorization"], "Bearer captured-key");
        assert_eq!(request.headers["content-type"], "application/json");
        assert_eq!(request.body["stream"], true);
        assert_eq!(request.body["prompt_cache_key"], session.to_string());
        assert!(request.body.get("tool_choice").is_none());
        assert!(request.body.get("previous_response_id").is_none());
        assert!(!request.body.to_string().contains("encrypted_content"));
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
    let event = json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":"signed-call","name":"read","args":{"path":"file"}},"thoughtSignature":"signed-thought"}]},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":3,"totalTokenCount":13},"modelVersion":"gemini-3.5-pro"});
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
    let config = ModelConfig::new("gemini-3.5-pro");
    let (mut call, _) = collect_stream(
        native
            .stream(&config, "", &[Message::user().with_text("Read")], &[tool()])
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    let request = call
        .content
        .iter_mut()
        .find_map(|block| match block {
            MessageContentBlock::ToolRequest(request) => Some(request),
            _ => None,
        })
        .unwrap();
    assert_eq!(request.id, "signed-call");
    assert_eq!(
        request.metadata.as_ref().unwrap()["thoughtSignature"],
        "signed-thought"
    );
    request.tool_meta = Some(json!({"title":"Retained metadata"}));
    let serialized = serde_json::to_value(&call).unwrap();
    let call: Message = serde_json::from_value(serialized).unwrap();
    let request = call
        .content
        .iter()
        .find_map(|block| block.as_tool_request())
        .unwrap();
    assert_eq!(
        request.tool_meta.as_ref().unwrap()["title"],
        "Retained metadata"
    );
    let result = Message::user().with_tool_response(
        "signed-call",
        Ok(CallToolResult::success(vec![Content::text("Done")])),
    );
    let _ = collect_stream(
        native
            .stream(&config, "", &[call, result], &[tool()])
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
                && part["thoughtSignature"] == "signed-thought")
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
        let config = ModelConfig::new("claude-sonnet-4-6")
            .with_max_tokens(Some(4096))
            .with_thinking_effort(ThinkingEffort::High);
        let (message, _) = collect_stream(
            native
                .stream(
                    &config,
                    "System",
                    &[Message::user().with_text("Read")],
                    &[tool()],
                )
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        assert!(message.content.iter().any(|block|matches!(block,MessageContentBlock::Thinking(thinking) if thinking.signature=="signed-thinking")));
        assert!(message.content.iter().any(|block|matches!(block,MessageContentBlock::RedactedThinking(thinking) if thinking.data=="redacted-data")));
        let result = Message::user().with_tool_response(
            "call",
            Ok(CallToolResult::success(vec![Content::text("Done")])),
        );
        let _ = collect_stream(
            native
                .stream(&config, "System", &[message, result], &[tool()])
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
    let _ = collect_stream(
        native
            .stream(
                &ModelConfig::new("deployment"),
                "",
                &[Message::user().with_text("Hello")],
                &[],
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
    let mut config = ModelConfig::new("model");
    config.request_headers = Some(
        [
            ("authorization".into(), "untrusted-override".into()),
            ("x-policy".into(), "captured".into()),
        ]
        .into(),
    );
    let messages = [Message::user().with_text("Hello")];
    let send = async |provider: Arc<dyn GdkProvider>| {
        collect_stream(provider.stream(&config, "", &messages, &[]).await.unwrap())
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
async fn uses_gdk_rate_limit_retry() {
    let rejected = "HTTP/1.1 429 Rate limited\r\nContent-Type: application/json\r\nRetry-After: 0\r\nContent-Length: 28\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"Wait\"}}";
    let server = Server::start(vec![rejected.into(), response(false)]).await;
    let registry = registry::Registry::default();
    let provider = provider(ModelApi::Responses, server.endpoint.clone());
    let native = native(
        &registry,
        &provider,
        "model",
        SessionId::new(),
        "captured",
        HeaderMap::new(),
    )
    .unwrap();
    let (message, _) = collect_stream(
        native
            .stream(
                &ModelConfig::new("model"),
                "",
                &[Message::user().with_text("Hello")],
                &[],
            )
            .await
            .unwrap(),
    )
    .await
    .unwrap();
    assert!(message.content.iter().any(
        |block| matches!(block, MessageContentBlock::Text(text) if text.text=="Read complete")
    ));
    assert_eq!(server.finish().await.len(), 2);
}

#[test]
fn maps_captured_effort_and_output() {
    let mut provider = provider(ModelApi::Responses, "https://example.test/v1".into());
    provider.models = serde_json::from_value(json!([{
        "id":"captured","context":100_000,"output":4096,"vision":true,"reasoning":true,"default_effort":"low"
    }])).unwrap();
    let mut session: SessionConfig = serde_json::from_value(json!({
        "provider":provider.id,"model":"captured","effort":"max","mode":"code","permission":"ask","credential":null
    })).unwrap();
    let frozen = config(&provider, &session);
    assert_eq!(frozen.model_name, "captured");
    assert_eq!(frozen.max_tokens, Some(4096));
    assert_eq!(frozen.context_limit, Some(100_000));
    assert_eq!(frozen.thinking_effort(), Some(ThinkingEffort::Max));
    session.effort = Effort::Low;
    let next = config(&provider, &session);
    assert_eq!(next.thinking_effort(), Some(ThinkingEffort::Low));
    assert_eq!(frozen.thinking_effort(), Some(ThinkingEffort::Max));
    session.effort = Effort::Disabled;
    assert_eq!(
        config(&provider, &session).thinking_effort(),
        Some(ThinkingEffort::Off)
    );
}
