use super::*;

#[test]
fn maps_domain_controls_without_mutual_exclusion() {
    let mut input = input();
    input.include_domains = vec!["example.com".into(), "docs.example.org".into()];
    input.exclude_domains = vec!["private.example.com".into()];
    let body = responses::body("fixture-model", &input);
    assert_eq!(
        body["tools"][0],
        json!({"type":"web_search", "filters":{
            "allowed_domains":["example.com","docs.example.org"],
            "blocked_domains":["private.example.com"]
        }})
    );
    assert_eq!(
        body["input"][0]["content"][0]["text"],
        "search fixture (site:example.com OR site:docs.example.org) -site:private.example.com"
    );
    let body = anthropic::body("fixture-model", &input);
    assert_eq!(
        body["tools"][0],
        json!({
            "type":"web_search_20250305","name":"web_search","max_uses":3,
            "allowed_domains":["example.com","docs.example.org"]
        })
    );
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("-site:private.example.com")
    );
    input.include_domains.clear();
    assert_eq!(
        anthropic::body("fixture-model", &input)["tools"][0]["blocked_domains"],
        json!(["private.example.com"])
    );
}

#[test]
fn maps_google_time_range_and_query_hints() {
    let mut input = input();
    input.freshness = Some(search::Freshness::Hour);
    input.include_domains = vec!["example.com".into()];
    input.exclude_domains = vec!["private.example.com".into()];
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
        .unwrap()
        .to_utc();
    assert_eq!(
        gemini::body(&input, now),
        json!({
            "contents":[{"role":"user","parts":[{"text":"search fixture site:example.com -site:private.example.com"}]}],
            "tools":[{"googleSearch":{"timeRangeFilter":{
                "startTime":"2026-10-30T11:00:00Z","endTime":"2026-10-30T12:00:00Z"
            }}}]
        })
    );
}

#[test]
fn preserves_configured_generation_routes() {
    for (api, base, model, expected) in [
        (
            ModelApi::Responses,
            "https://api.openai.com/v1",
            "gpt-model",
            "https://api.openai.com/v1/responses",
        ),
        (
            ModelApi::Responses,
            "https://chatgpt.com/backend-api/codex",
            "gpt-model",
            "https://chatgpt.com/backend-api/codex/responses",
        ),
        (
            ModelApi::Responses,
            "https://gateway.test/proxy/responses",
            "gpt-model",
            "https://gateway.test/proxy/responses",
        ),
        (
            ModelApi::Anthropic,
            "https://api.anthropic.com/",
            "claude-model",
            "https://api.anthropic.com/v1/messages",
        ),
        (
            ModelApi::Anthropic,
            "https://gateway.test/proxy/v1/messages",
            "claude-model",
            "https://gateway.test/proxy/v1/messages",
        ),
        (
            ModelApi::Gemini,
            "https://generativelanguage.googleapis.com",
            "models/gemini-model",
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-model:generateContent",
        ),
        (
            ModelApi::Gemini,
            "https://gateway.test/proxy/v1beta",
            "gemini-model",
            "https://gateway.test/proxy/v1beta/models/gemini-model:generateContent",
        ),
    ] {
        assert_eq!(endpoint(api, base, model).unwrap().as_str(), expected);
    }
    for model in ["../escape", "gemini/model", "gemini:otherMethod", ""] {
        assert!(endpoint(ModelApi::Gemini, "https://gateway.test", model).is_err());
    }
}

#[test]
fn preserves_api_key_injection() {
    let mut provider = auth::provider(Authentication::ApiKey, ModelApi::Responses);
    provider.binding = Some(Box::new(sailry_protocol::conversation::presets::Binding {
        key_injection: Some(sailry_protocol::plugin::model_auth::Injection {
            header: "x-customer-key".into(),
            template: "Token {key}".into(),
        }),
        ..Default::default()
    }));
    let prepared = prepare(
        &provider,
        &Credentials::ApiKey(Some(Secret::new("fixed-access".into()))),
        "fixture-model",
        auth::SESSION.parse().unwrap(),
        auth::REQUEST.parse().unwrap(),
        &input(),
        true,
    )
    .unwrap();
    assert_eq!(
        prepared.authorization.headers["x-customer-key"],
        "Token fixed-access"
    );
    assert!(!prepared.authorization.headers.contains_key("authorization"));
}
