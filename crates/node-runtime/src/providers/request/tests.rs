use super::*;

#[test]
fn matches_captured_authorization_headers() {
    // Fixed baselines from the pre-refactor Grant headers and stream policy.
    for authentication in [
        Authentication::ApiKey,
        Authentication::ChatGpt,
        Authentication::ChatGptOAuth,
        Authentication::ClaudeOAuth,
        Authentication::Copilot,
        Authentication::Plugin,
    ] {
        let provider = fixture::provider(authentication, ModelApi::Responses);
        let credentials = fixture::credentials(authentication);
        let prepared = prepare(
            &provider,
            &credentials,
            fixture::metadata("fixture-model", "Captured system", &["read", "custom"]),
        )
        .unwrap();
        assert_eq!(prepared.endpoint, "https://model.example.test/gateway");
        assert_eq!(
            fixture::header_value(&prepared.headers),
            fixture::headers(authentication)
        );
        for transient in [
            "session-id",
            "x-codex-routing-hint",
            "openai-intent",
            "x-initiator",
            "x-request-id",
            "copilot-vision-request",
        ] {
            assert!(!prepared.cached_headers.contains_key(transient));
        }
        for secret in ["fixed-access", "fixed-refresh"] {
            assert!(
                prepared
                    .secrets
                    .iter()
                    .any(|value| value.expose().contains(secret))
                    || matches!(
                        authentication,
                        Authentication::ApiKey | Authentication::Copilot
                    ) && secret == "fixed-refresh"
            );
        }
    }
}

#[test]
fn preserves_key_protocol_headers_and_injection() {
    for (api, name) in [
        (ModelApi::Responses, "authorization"),
        (ModelApi::ChatCompletions, "authorization"),
        (ModelApi::Anthropic, "x-api-key"),
        (ModelApi::Gemini, "x-goog-api-key"),
        (ModelApi::AzureAi, "api-key"),
        (ModelApi::AzureOpenAi, "api-key"),
    ] {
        let mut provider = fixture::provider(Authentication::ApiKey, api);
        let credentials = fixture::credentials(Authentication::ApiKey);
        let prepared = prepare(
            &provider,
            &credentials,
            fixture::metadata("fixture-model", "", &[]),
        )
        .unwrap();
        assert_eq!(
            prepared.headers[name],
            if name == "authorization" {
                "Bearer fixed-access"
            } else {
                "fixed-access"
            }
        );
        assert!(prepared.headers[name].is_sensitive());
        provider.binding = Some(serde_json::from_value(json!({"package":null, "key_injection":{"header":"x-captured-key", "template":"key {key}"}})).unwrap());
        let prepared = prepare(
            &provider,
            &credentials,
            fixture::metadata("fixture-model", "", &[]),
        )
        .unwrap();
        assert_eq!(prepared.headers["x-captured-key"], "key fixed-access");
        let empty = headers(&provider, &Credentials::ApiKey(None)).unwrap();
        assert!(!empty.contains_key("x-captured-key"));
    }
}

#[test]
fn separates_google_provider_and_native_headers() {
    let provider = fixture::provider(Authentication::ApiKey, ModelApi::Gemini);
    assert!(
        matches!(auth(&provider, "fixed-access"), KeyAuth::Bearer(key) if key == "fixed-access")
    );
    let prepared = prepare(
        &provider,
        &fixture::credentials(Authentication::ApiKey),
        fixture::metadata("fixture-model", "", &[]),
    )
    .unwrap();
    assert_eq!(prepared.headers["x-goog-api-key"], "fixed-access");
    assert!(!prepared.headers.contains_key("authorization"));
}

#[test]
fn scopes_copilot_transient_headers_to_each_request() {
    let provider = fixture::provider(Authentication::Copilot, ModelApi::Responses);
    let credentials = fixture::credentials(Authentication::Copilot);
    for (delegated, user, initiator) in [
        (false, true, "user"),
        (true, true, "agent"),
        (false, false, "agent"),
    ] {
        let mut metadata = fixture::metadata("fixture-model", "", &[]);
        metadata.delegated = delegated;
        metadata.user = user;
        metadata.vision = false;
        let first = prepare(&provider, &credentials, metadata).unwrap();
        let mut next = fixture::metadata("fixture-model", "", &[]);
        next.request = RequestId::new();
        let second = prepare(&provider, &credentials, next).unwrap();
        assert_eq!(first.headers["x-initiator"], initiator);
        assert!(!first.headers.contains_key("copilot-vision-request"));
        assert_ne!(
            first.headers["x-request-id"],
            second.headers["x-request-id"]
        );
        assert!(!first.cached_headers.contains_key("x-request-id"));
    }
}

#[test]
fn applies_fixed_json_baselines() {
    for authentication in [
        Authentication::ApiKey,
        Authentication::ChatGpt,
        Authentication::ChatGptOAuth,
        Authentication::Plugin,
    ] {
        let provider = fixture::provider(authentication, ModelApi::Responses);
        let credentials = fixture::credentials(authentication);
        let prepared = prepare(
            &provider,
            &credentials,
            fixture::metadata("fixture-model", "Captured system", &[]),
        )
        .unwrap();
        let mut body = json!({"model":"fixture-model", "input":[{"role":"user", "content":"Task"}], "max_output_tokens":64, "temperature":0.25});
        prepared.apply_json(&mut body).unwrap();
        let expected = if authentication == Authentication::ChatGptOAuth {
            json!({"model":"fixture-model", "input":[{"role":"user", "content":"Task"}], "instructions":"Captured system", "prompt_cache_key":fixture::SESSION})
        } else if authentication == Authentication::ChatGpt {
            json!({"model":"fixture-model", "input":[{"role":"user", "content":"Task"}], "instructions":"Captured system", "store":false, "stream":true})
        } else {
            json!({"model":"fixture-model", "input":[{"type":"message", "role":"system", "content":[{"type":"input_text", "text":"Captured system"}]}, {"role":"user", "content":"Task"}], "max_output_tokens":64, "temperature":0.25})
        };
        assert_eq!(body, expected);
    }
}

#[test]
fn conforms_to_chatgpt_preview_limits() {
    let provider = fixture::provider(Authentication::ChatGpt, ModelApi::Responses);
    let credentials = fixture::credentials(Authentication::ChatGpt);
    let prepared = prepare(
        &provider,
        &credentials,
        fixture::metadata("fixture-model", "Captured system", &[]),
    )
    .unwrap();
    // The GDK policy remains the pre-refactor baseline; only direct JSON adapts SIWC.
    assert_eq!(prepared.system.placement, Placement::Native);
    assert!(prepared.parameters.is_empty());
    assert_eq!(prepared.remove_params, &["max_tokens", "temperature"]);
    let mut body = json!({
        "model":"fixture-model",
        "input":[
            {"type":"message", "role":"system", "content":[{"type":"input_text", "text":"Old system"}]},
            {"role":"developer", "content":"Additional guidance"},
            {"role":"user", "content":"Task"}
        ],
        "instructions":"Old instructions", "store":true, "stream":false,
        "background":false, "conversation":"conversation-id", "max_tokens":64,
        "max_output_tokens":64, "max_tool_calls":2, "metadata":{"tag":"fixture"},
        "moderation":{}, "multi_agent":{}, "prompt":{}, "prompt_cache_retention":"24h",
        "safety_identifier":"fixture", "temperature":0.25, "top_logprobs":1,
        "top_p":0.9, "truncation":"auto", "user":"fixture", "previous_response_id":"old-response",
        "tools":[{"type":"web_search"}], "tool_choice":"required",
        "include":["web_search_call.action.sources"], "prompt_cache_key":"fixed-cache",
        "reasoning":{"effort":"low"}
    });
    prepared.apply_json(&mut body).unwrap();
    assert_eq!(
        body,
        json!({
            "model":"fixture-model",
            "input":[
                {"role":"developer", "content":"Additional guidance"},
                {"role":"user", "content":"Task"}
            ],
            "instructions":"Captured system", "store":false, "stream":true,
            "tools":[{"type":"web_search"}], "tool_choice":"required",
            "include":["web_search_call.action.sources"], "prompt_cache_key":"fixed-cache",
            "reasoning":{"effort":"low"}
        })
    );
}

#[test]
fn preserves_server_tools_and_maps_function_history() {
    let provider = fixture::provider(Authentication::ClaudeOAuth, ModelApi::Anthropic);
    let credentials = fixture::credentials(Authentication::ClaudeOAuth);
    let prepared = prepare(
        &provider,
        &credentials,
        fixture::metadata("fixture-model", "Captured system", &["read", "web_search"]),
    )
    .unwrap();
    let mut body = json!({
        "model":"fixture-model", "tools":[{"name":"read", "input_schema":{}}, {"type":"web_search_20250305", "name":"web_search"}],
        "messages":[{"role":"assistant", "content":[{"type":"tool_use", "name":"read", "id":"call", "input":{}}]}]
    });
    prepared.apply_json(&mut body).unwrap();
    assert_eq!(
        body,
        json!({
            "model":"fixture-model", "tools":[{"name":"Read", "input_schema":{}}, {"type":"web_search_20250305", "name":"web_search"}],
            "system":"You are Claude Code, Anthropic's official CLI for Claude.\n\nCaptured system",
            "messages":[{"role":"assistant", "content":[{"type":"tool_use", "name":"Read", "id":"call", "input":{}}]}]
        })
    );
    assert_eq!(
        prepared.names.reverse.get("Read").map(String::as_str),
        Some("read")
    );
    assert!(!prepared.names.reverse.contains_key("READ"));
}

#[test]
fn retains_plugin_binding_validation() {
    let provider = fixture::provider(Authentication::Plugin, ModelApi::Responses);
    let mut credentials = fixture::credentials(Authentication::Plugin);
    let Credentials::Grant {
        options: Options::Plugin { endpoint, .. },
        ..
    } = &mut credentials
    else {
        unreachable!()
    };
    *endpoint = "https://different.example.test".into();
    let error = prepare(
        &provider,
        &credentials,
        fixture::metadata("fixture-model", "", &[]),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    for secret in ["fixed-access", "fixed-refresh", "private-account"] {
        assert!(!error.message.contains(secret));
    }
}
