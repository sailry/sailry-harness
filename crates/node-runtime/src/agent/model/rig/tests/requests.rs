use super::*;
use sailry_protocol::{Effort, conversation::connection::Options};

#[tokio::test]
async fn preserves_anthropic_cache_breakpoints() {
    let server = Server::start(vec![fixtures::anthropic(false)]).await;
    let provider = provider(ModelApi::Anthropic, server.endpoint.clone());
    let model = native(&provider, "claude-opus-4-8", "key", HeaderMap::new());
    let mut input = request("claude-opus-4-8");
    input.messages = (0..5)
        .map(|index| Message {
            role: if index % 2 == 0 {
                Role::User
            } else {
                Role::Assistant
            },
            content: vec![Content::Text {
                text: format!("Message {index}"),
            }],
        })
        .collect();
    collect(&model, input, CancellationToken::new())
        .await
        .unwrap();
    let sent = server.finish().await;
    let body = &sent[0].body;
    assert_eq!(
        body["tools"][0]["cache_control"],
        json!({"type":"ephemeral"})
    );
    assert!(body["tools"][0].get("eager_input_streaming").is_none());
    assert_eq!(
        body["system"][0]["cache_control"],
        json!({"type":"ephemeral"})
    );
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 5);
    for (index, message) in messages.iter().enumerate() {
        assert_eq!(
            message["content"][0].get("cache_control").is_some(),
            matches!(index, 2 | 4)
        );
    }
}

#[tokio::test]
async fn preserves_responses_configuration() {
    for tier in ["priority", "ultrafast"] {
        let server = Server::start(vec![fixtures::responses(false)]).await;
        let provider = provider(
            ModelApi::Responses,
            format!("{}/gateway/v1", server.endpoint),
        );
        let model = native(&provider, "fixture", "captured-key", HeaderMap::new());
        let mut input = request("fixture");
        input.effort = Effort::XHigh;
        input.params = json!({
            "service_tier":tier,"prompt_cache_key":"captured-session",
            "reasoning":{"effort":"xhigh","summary":"auto"}
        })
        .as_object()
        .unwrap()
        .clone();
        input.messages[0].content.push(Content::Image {
            media_type: "image/png".into(),
            data: Image::Inline {
                data_base64: "aW1hZ2U=".into(),
            },
            annotations: None,
        });
        let events = collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(text(&events), fixtures::ANSWER);
        let captured = server.finish().await;
        assert_eq!(captured.len(), 1);
        let sent = &captured[0];
        assert_eq!(sent.path, "/gateway/v1/responses");
        assert_eq!(sent.headers["authorization"], "Bearer captured-key");
        assert_eq!(sent.body["stream"], true);
        assert_eq!(sent.body["service_tier"], tier);
        assert_eq!(sent.body["prompt_cache_key"], "captured-session");
        assert_eq!(sent.body["reasoning"]["effort"], "xhigh");
        assert_eq!(sent.body["reasoning"]["summary"], "auto");
        assert_eq!(sent.body["instructions"], "Captured instructions");
        assert!(
            sent.body["input"]
                .to_string()
                .contains("data:image/png;base64,aW1hZ2U=")
        );
        assert_eq!(sent.body["tools"][0]["name"], "read");
        assert_eq!(
            sent.body["tools"][0]["parameters"]["properties"]["path"]["type"],
            "string"
        );
        assert!(sent.body.get("tool_choice").is_none());
        assert!(sent.body.get("previous_response_id").is_none());
    }
}

#[tokio::test]
async fn accepts_compatible_endpoints() {
    for suffix in ["/gateway/v1", "/gateway/v1/chat/completions"] {
        let server = Server::start(vec![fixtures::chat()]).await;
        let provider = provider(
            ModelApi::ChatCompletions,
            format!("{}{suffix}", server.endpoint),
        );
        let model = native(
            &provider,
            "uncatalogued",
            "compatible-key",
            HeaderMap::new(),
        );
        let mut input = request("uncatalogued");
        input.params = json!({
            "reasoning_effort":"high","prompt_cache_key":"captured-session"
        })
        .as_object()
        .unwrap()
        .clone();
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        assert_eq!(captured.len(), 1);
        let sent = &captured[0];
        assert_eq!(sent.path, "/gateway/v1/chat/completions");
        assert_eq!(sent.headers["authorization"], "Bearer compatible-key");
        assert_eq!(sent.body["model"], "uncatalogued");
        assert_eq!(sent.body["reasoning_effort"], "high");
        assert_eq!(sent.body["prompt_cache_key"], "captured-session");
        assert_eq!(sent.body["tools"][0]["function"]["name"], "read");
        assert!(sent.body.get("tool_choice").is_none());
    }
}

#[tokio::test]
async fn accepts_captured_codex_authorization() {
    let server = Server::start(vec![fixtures::responses(false)]).await;
    let mut provider = provider(
        ModelApi::Responses,
        format!("{}/backend-api/codex", server.endpoint),
    );
    provider.authentication = Authentication::ChatGptOAuth;
    let session = SessionId::new();
    let headers = [
        ("authorization", "Bearer captured-access".to_owned()),
        ("chatgpt-account-id", "captured-account".to_owned()),
        ("originator", "codex_cli_rs".to_owned()),
        ("session-id", session.to_string()),
        ("x-codex-routing-hint", "model=fixture".to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| {
        (
            reqwest::header::HeaderName::from_static(name),
            reqwest::header::HeaderValue::from_str(&value).unwrap(),
        )
    })
    .collect();
    let model = RigModel::new(
        &registry::Registry::default(),
        &provider,
        "fixture",
        session,
        "captured-access",
        headers,
    )
    .unwrap();
    let mut input = request("fixture");
    input.max_tokens = None;
    input.params = json!({
        "store":false,"prompt_cache_key":session.to_string(),
        "include":["reasoning.encrypted_content"]
    })
    .as_object()
    .unwrap()
    .clone();
    collect(&model, input, CancellationToken::new())
        .await
        .unwrap();
    let captured = server.finish().await;
    assert_eq!(captured.len(), 1);
    let sent = &captured[0];
    assert_eq!(sent.path, "/backend-api/codex/responses");
    assert_eq!(sent.headers["authorization"], "Bearer captured-access");
    assert_eq!(sent.headers["chatgpt-account-id"], "captured-account");
    assert_eq!(sent.headers["originator"], "codex_cli_rs");
    assert_eq!(sent.headers["session-id"], session.to_string());
    assert_eq!(sent.headers["x-codex-routing-hint"], "model=fixture");
    assert_eq!(sent.body["store"], false);
    assert_eq!(sent.body["prompt_cache_key"], session.to_string());
    assert_eq!(sent.body["include"], json!(["reasoning.encrypted_content"]));
    for field in [
        "max_output_tokens",
        "previous_response_id",
        "stream_options",
        "temperature",
    ] {
        assert!(sent.body.get(field).is_none(), "{field}");
    }
    assert!(
        sent.body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["type"] == "function")
    );
    assert!(!sent.body.to_string().contains("captured-access"));
}

#[tokio::test]
async fn accepts_captured_copilot_authorization() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let reply = if api == ModelApi::Responses {
            fixtures::responses(false)
        } else {
            fixtures::chat()
        };
        let server = Server::start(vec![reply]).await;
        let mut provider = provider(api, format!("{}/captured", server.endpoint));
        provider.authentication = Authentication::Copilot;
        let headers = [
            ("authorization", "Bearer exchanged-token"),
            ("copilot-integration-id", "vscode-chat"),
            ("x-initiator", "agent"),
            ("editor-version", "vscode/1.99.0"),
            ("x-request-id", "captured-request"),
        ]
        .into_iter()
        .map(|(name, value)| {
            (
                reqwest::header::HeaderName::from_static(name),
                reqwest::header::HeaderValue::from_static(value),
            )
        })
        .collect();
        let model = native(&provider, "fixture", "exchanged-token", headers);
        collect(&model, request("fixture"), CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        assert_eq!(captured.len(), 1);
        let sent = &captured[0];
        assert_eq!(
            sent.path,
            if api == ModelApi::Responses {
                "/captured/responses"
            } else {
                "/captured/chat/completions"
            }
        );
        assert_eq!(sent.headers["authorization"], "Bearer exchanged-token");
        assert_eq!(sent.headers["copilot-integration-id"], "vscode-chat");
        assert_eq!(sent.headers["x-initiator"], "agent");
        assert_eq!(sent.headers["editor-version"], "vscode/1.99.0");
        assert_eq!(sent.headers["x-request-id"], "captured-request");
        assert!(sent.body.get("service_tier").is_none());
        assert!(!sent.body.to_string().contains("exchanged-token"));
    }
}

#[tokio::test]
async fn preserves_adaptive_anthropic_parameters() {
    for suffix in ["/v1", "/v1/messages", "/proxy/v1", "/proxy/v1/messages"] {
        let server = Server::start(vec![fixtures::anthropic(false)]).await;
        let provider = provider(ModelApi::Anthropic, format!("{}{suffix}", server.endpoint));
        let model = native(&provider, "claude-opus-4-8", "claude-key", HeaderMap::new());
        let mut input = request("claude-opus-4-8");
        input.max_tokens = Some(16000);
        input.effort = Effort::XHigh;
        input.params = json!({
            "thinking":{"type":"adaptive"},"output_config":{"effort":"xhigh"}
        })
        .as_object()
        .unwrap()
        .clone();
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        assert_eq!(captured.len(), 1);
        let sent = &captured[0];
        assert_eq!(
            sent.path,
            if suffix.starts_with("/proxy") {
                "/proxy/v1/messages"
            } else {
                "/v1/messages"
            }
        );
        assert_eq!(sent.headers["x-api-key"], "claude-key");
        assert_eq!(sent.headers["anthropic-version"], "2023-06-01");
        assert_eq!(sent.body["thinking"]["type"], "adaptive");
        assert_eq!(sent.body["output_config"]["effort"], "xhigh");
        assert_eq!(sent.body["max_tokens"], 16000);
        assert_eq!(sent.body["tools"][0]["name"], "read");
        assert_eq!(
            sent.body["tools"][0]["input_schema"]["required"],
            json!(["path"])
        );
    }
}

#[tokio::test]
async fn retains_gemini_schema_and_images() {
    let server = Server::start(vec![fixtures::gemini(false)]).await;
    let provider = provider(
        ModelApi::Gemini,
        format!("{}/proxy/v1beta", server.endpoint),
    );
    let model = native(
        &provider,
        "gemini-3-flash-preview",
        "google-key",
        HeaderMap::new(),
    );
    let mut input = request("gemini-3-flash-preview");
    input.effort = Effort::High;
    input.params = json!({
        "generationConfig":{"thinkingConfig":{"thinkingLevel":"high","includeThoughts":true}}
    })
    .as_object()
    .unwrap()
    .clone();
    input.messages[0].content.push(Content::Image {
        media_type: "image/png".into(),
        data: Image::Inline {
            data_base64: "aW1hZ2U=".into(),
        },
        annotations: None,
    });
    let schema = json!({
        "type":"object","properties":{
            "action":{"type":"string","enum":["read","write"]},
            "path":{"type":"string"}
        },
        "required":["action"],
        "oneOf":[
            {"properties":{"action":{"const":"read"}}},
            {"properties":{"action":{"const":"write"}},"required":["path"]}
        ]
    });
    input.tools[0].parameters = schema.clone();
    collect(&model, input, CancellationToken::new())
        .await
        .unwrap();
    let captured = server.finish().await;
    assert_eq!(captured.len(), 1);
    let sent = &captured[0];
    assert_eq!(
        sent.path,
        "/proxy/v1beta/models/gemini-3-flash-preview:streamGenerateContent?alt=sse"
    );
    assert_eq!(sent.headers["x-goog-api-key"], "google-key");
    assert!(!sent.headers.contains_key("authorization"));
    let tools = if sent.body["tools"].is_array() {
        &sent.body["tools"][0]
    } else {
        &sent.body["tools"]
    };
    assert_eq!(
        tools["functionDeclarations"][0]["parametersJsonSchema"],
        schema
    );
    assert_eq!(
        sent.body["generationConfig"]["thinkingConfig"],
        json!({"thinkingLevel":"high","includeThoughts":true})
    );
    assert_eq!(sent.body["generationConfig"]["maxOutputTokens"], 512);
    assert!(
        sent.body["contents"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|content| content["parts"].as_array().unwrap())
            .any(|part| part["inlineData"]["mimeType"] == "image/png"
                && part["inlineData"]["data"] == "aW1hZ2U=")
    );
}

#[tokio::test]
async fn keeps_azure_deployment_and_version() {
    let server = Server::start(vec![fixtures::chat()]).await;
    let mut provider = provider(ModelApi::AzureOpenAi, server.endpoint.clone());
    provider.options = Some(Options::AzureOpenAi {
        api_version: "2025-01-01-preview".into(),
    });
    let model = native(&provider, "deployment", "azure-key", HeaderMap::new());
    collect(&model, request("deployment"), CancellationToken::new())
        .await
        .unwrap();
    let captured = server.finish().await;
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0].path,
        "/openai/deployments/deployment/chat/completions?api-version=2025-01-01-preview"
    );
    assert_eq!(captured[0].headers["api-key"], "azure-key");
    assert!(!captured[0].headers.contains_key("authorization"));
}

#[tokio::test]
async fn keeps_foundry_maas_routes() {
    for suffix in ["/v1", "/v1/chat/completions", "/proxy/v1"] {
        let server = Server::start(vec![fixtures::chat()]).await;
        let provider = provider(ModelApi::AzureAi, format!("{}{suffix}", server.endpoint));
        let model = native(&provider, "fixture", "foundry-key", HeaderMap::new());
        let mut input = request("fixture");
        input.params.insert("max_tokens".into(), json!(512));
        collect(&model, input, CancellationToken::new())
            .await
            .unwrap();
        let captured = server.finish().await;
        assert_eq!(captured.len(), 1);
        assert_eq!(
            captured[0].path,
            if suffix.starts_with("/proxy") {
                "/proxy/v1/chat/completions?api-version=2024-05-01-preview"
            } else {
                "/v1/chat/completions?api-version=2024-05-01-preview"
            }
        );
        assert_eq!(captured[0].headers["api-key"], "foundry-key");
        assert_eq!(captured[0].body["model"], "fixture");
        assert_eq!(captured[0].body["max_tokens"], 512);
    }
}
