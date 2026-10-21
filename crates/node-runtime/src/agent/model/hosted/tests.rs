use super::*;
use sailry_protocol::{
    ProviderId,
    conversation::connection::{Options, Product},
};
use serde_json::Value;

fn provider() -> Provider {
    Provider {
        preset: None,
        binding: None,
        consent: None,
        id: ProviderId::new(),
        revision: 1,
        name: "Fixture".into(),
        api: ModelApi::ChatCompletions,
        authentication: Authentication::ApiKey,
        endpoint: "http://127.0.0.1:9/v1".into(),
        options: None,
        oauth: None,
        enabled: true,
        models: vec![],
        default_model: String::new(),
        credential: None,
    }
}

#[test]
fn identifies_sailry() {
    let headers = headers();
    assert_eq!(headers.len(), 1);
    assert_eq!(
        headers[USER_AGENT],
        concat!("Sailry/", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn selects_documented_fields() {
    let mut provider = provider();
    for options in [
        None,
        Some(Options::Hosted {
            product: Product::Coding,
        }),
        Some(Options::Hosted {
            product: Product::Metered,
        }),
    ] {
        provider.options = options;
        for model in [
            "qwen3.6-plus",
            "qwen3.6-plus-2026-04-02",
            "kimi-for-coding",
            "kimi-k2.6",
            "kimi-k2.7-code",
            "kimi-k2.7-code-highspeed",
            "kimi-k3",
            "glm-4.7",
            "glm-5",
            "glm-5.1",
            "glm-5.2",
            "glm-5.3",
            "glm-5.3-flash",
            "MiniMax-M3.1-Flash-Preview",
            "doubao-seed-2-0-pro-260215",
            "doubao-seed-2-0-lite-260215",
            "doubao-seed-2-0-mini-260215",
            "deepseek-v4-pro-260425",
            "deepseek-v4-flash-ga-260731",
            "deepseek-v4-1-flash-260910",
            "glm-5-2-260617",
        ] {
            assert!(replay(&provider, model), "{model}");
        }
    }
}

#[test]
fn leaves_other_contracts_unchanged() {
    let mut provider = provider();
    provider.options = Some(Options::Hosted {
        product: Product::Coding,
    });
    for model in [
        "fixture-a",
        "qwen-new",
        "qwen3-coder-plus",
        "kimi-new",
        "glm-new",
        "MiniMax-M3",
        "MiniMax-M2.5",
        "ark-code-latest",
        "doubao-seed-evolving",
        "doubao-seed-2-1-pro-260915",
    ] {
        assert!(!replay(&provider, model), "{model}");
        assert!(adapter(&provider, model).is_none(), "{model}");
    }
    for authentication in [
        Authentication::ChatGpt,
        Authentication::ChatGptOAuth,
        Authentication::ClaudeOAuth,
        Authentication::Copilot,
    ] {
        provider.authentication = authentication;
        assert!(!replay(&provider, "qwen3.6-plus"));
        assert!(adapter(&provider, "qwen3.6-plus").is_none());
    }
    provider.authentication = Authentication::ApiKey;
    for api in [
        ModelApi::Anthropic,
        ModelApi::Responses,
        ModelApi::DeepSeek,
        ModelApi::Gemini,
        ModelApi::OpenCodeGo,
        ModelApi::OpenCodeZen,
        ModelApi::AzureAi,
        ModelApi::AzureOpenAi,
    ] {
        provider.api = api;
        assert!(!replay(&provider, "glm-5.2"));
        assert!(adapter(&provider, "glm-5.2").is_none());
    }
}

#[test]
fn preserves_request_choices() {
    let provider = provider();
    for model in ["qwen3.6-plus", "glm-5.2"] {
        for effort in [None, Some("none"), Some("high")] {
            let mut body = json!({"model":model,"stream":true,"messages":[{"role":"user","content":"Inspect the file"}],"max_tokens":2048,"temperature":0.2,"thinking":{"type":"disabled"}});
            if let Some(effort) = effort {
                body["reasoning_effort"] = json!(effort);
            }
            let original = body.clone();
            let mut headers = HeaderMap::from_iter([(
                "authorization".parse().unwrap(),
                HeaderValue::from_static("Bearer fixture"),
            )]);
            let original_headers = headers.clone();
            adapter(&provider, model).unwrap()(&mut body, &mut headers).unwrap();
            assert_eq!(headers, original_headers);
            if model.starts_with("qwen") {
                assert_eq!(body["preserve_thinking"], true);
                body.as_object_mut().unwrap().remove("preserve_thinking");
            } else {
                assert_eq!(body["thinking"]["clear_thinking"], false);
                body["thinking"]
                    .as_object_mut()
                    .unwrap()
                    .remove("clear_thinking");
            }
            assert_eq!(body, original);
        }
    }
}

#[test]
fn retains_default_thinking_behavior() {
    let provider = provider();
    for model in ["qwen3.6-plus", "glm-5.2"] {
        let mut body = json!({"model":model});
        adapter(&provider, model).unwrap()(&mut body, &mut HeaderMap::new()).unwrap();
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("enable_thinking").is_none());
        assert!(body["thinking"].get("type").is_none());
    }
}

#[test]
fn rejects_malformed_parameters() {
    let provider = provider();
    for (model, mut body) in [
        ("qwen3.6-plus", Value::Null),
        ("glm-5.2", json!({"thinking":false})),
    ] {
        assert!(adapter(&provider, model).unwrap()(&mut body, &mut HeaderMap::new()).is_err());
    }
}

mod cache {
    use super::*;

    #[test]
    fn keeps_identity_and_request() {
        let session = SessionId::new();
        for api in [ModelApi::Responses, ModelApi::ChatCompletions] {
            let mut provider = provider();
            provider.api = api;
            let adapter = session_adapter(&provider, "fixture", session, false).unwrap();
            let mut original = json!({"model":"fixture", "stream":true, "temperature":0.2});
            let field = if api == ModelApi::Responses {
                "input"
            } else {
                "messages"
            };
            original[field] = json!([{"role":"user", "content":"Inspect the file"}]);
            for _ in 0..2 {
                let mut body = original.clone();
                let mut headers = headers();
                headers.insert("authorization", HeaderValue::from_static("Bearer fixture"));
                let original_headers = headers.clone();
                adapter(&mut body, &mut headers).unwrap();
                assert_eq!(body["prompt_cache_key"], session.to_string());
                assert_eq!(headers["session-id"], session.to_string());
                assert_eq!(headers["session_id"], session.to_string());
                body.as_object_mut().unwrap().remove("prompt_cache_key");
                body.as_object_mut().unwrap().remove("store");
                headers.remove("session-id");
                headers.remove("session_id");
                assert_eq!(body, original);
                assert_eq!(headers, original_headers);
            }
        }
    }

    #[test]
    fn separates_sessions() {
        let provider = provider();
        let mut keys = Vec::new();
        for session in [SessionId::new(), SessionId::new()] {
            let mut body = json!({"model":"fixture"});
            let mut headers = HeaderMap::new();
            session_adapter(&provider, "fixture", session, false).unwrap()(&mut body, &mut headers)
                .unwrap();
            keys.push(body["prompt_cache_key"].clone());
            assert_eq!(headers["session-id"], session.to_string());
            assert_eq!(headers["session_id"], session.to_string());
        }
        assert_ne!(keys[0], keys[1]);
    }

    #[test]
    fn composes_reasoning_fields() {
        let provider = provider();
        let session = SessionId::new();
        for model in ["qwen3.6-plus", "glm-5.2"] {
            let mut body = json!({"model":model, "thinking":{"type":"disabled"}});
            session_adapter(&provider, model, session, false).unwrap()(
                &mut body,
                &mut HeaderMap::new(),
            )
            .unwrap();
            assert_eq!(body["prompt_cache_key"], session.to_string());
            assert_eq!(body["thinking"]["type"], "disabled");
            if model.starts_with("qwen") {
                assert_eq!(body["preserve_thinking"], true);
            } else {
                assert_eq!(body["thinking"]["clear_thinking"], false);
            }
        }
    }

    #[test]
    fn leaves_other_contracts_unchanged() {
        let mut provider = provider();
        let session = SessionId::new();
        for authentication in [
            Authentication::ChatGpt,
            Authentication::ChatGptOAuth,
            Authentication::ClaudeOAuth,
            Authentication::Copilot,
        ] {
            provider.authentication = authentication;
            assert!(session_adapter(&provider, "fixture", session, false).is_none());
        }
        provider.authentication = Authentication::ApiKey;
        for api in [
            ModelApi::Anthropic,
            ModelApi::Gemini,
            ModelApi::DeepSeek,
            ModelApi::OpenCodeGo,
            ModelApi::OpenCodeZen,
            ModelApi::AzureOpenAi,
            ModelApi::AzureAi,
        ] {
            provider.api = api;
            assert!(session_adapter(&provider, "fixture", session, false).is_none());
        }
    }

    #[test]
    fn rejects_invalid_body() {
        let provider = provider();
        let adapter = session_adapter(&provider, "fixture", SessionId::new(), false).unwrap();
        assert!(adapter(&mut Value::Null, &mut HeaderMap::new()).is_err());
    }
}

#[test]
fn freezes_policy_and_preserves_builtin_scope() {
    let mut native = provider();
    let original = crate::providers::presets::bundled().policies;
    native.binding = Some(Box::new(sailry_protocol::conversation::presets::Binding {
        policies: original.clone(),
        ..Default::default()
    }));
    for model in original
        .iter()
        .flat_map(|policy| policy.models.iter())
        .map(String::as_str)
        .chain(["unknown"])
    {
        let mut legacy = provider();
        let expected = match model {
            "qwen3.6-plus" | "qwen3.6-plus-2026-04-02" => {
                json!({"model":model,"preserve_thinking":true})
            }
            "glm-4.7" | "glm-5" | "glm-5.1" | "glm-5.2" | "glm-5.3" | "glm-5.3-flash" => {
                json!({"model":model,"thinking":{"clear_thinking":false}})
            }
            _ => json!({"model":model}),
        };
        for selected in [&native, &legacy] {
            let mut body = json!({"model":model});
            if let Some(adapter) = adapter(selected, model) {
                adapter(&mut body, &mut HeaderMap::new()).unwrap();
            }
            assert_eq!(body, expected);
            assert_eq!(replay(selected, model), model != "unknown");
        }
        legacy.api = ModelApi::Responses;
        assert!(adapter(&legacy, model).is_none());
        assert!(!replay(&legacy, model));
    }
    let mut override_policy = original[0].clone();
    override_policy
        .body
        .insert("preserve_thinking".into(), false.into());
    native
        .binding
        .as_mut()
        .unwrap()
        .policies
        .push(override_policy);
    let mut body = json!({"model":"qwen3.6-plus","messages":[],"stream":false,"custom":7});
    adapter(&native, "qwen3.6-plus").unwrap()(&mut body, &mut HeaderMap::new()).unwrap();
    assert_eq!(
        body,
        json!({"model":"qwen3.6-plus","messages":[],"stream":false,"custom":7,"preserve_thinking":false})
    );
}

#[test]
fn captures_stateless_reasoning() {
    let mut provider = provider();
    provider.api = ModelApi::Responses;
    let adapter = session_adapter(&provider, "fixture", SessionId::new(), true).unwrap();
    let mut body = json!({"input":[],"include":["web_search_call.action.sources"],"previous_response_id":"untrusted"});
    adapter(&mut body, &mut HeaderMap::new()).unwrap();
    adapter(&mut body, &mut HeaderMap::new()).unwrap();
    assert_eq!(body["store"], false);
    assert_eq!(
        body["include"],
        json!([
            "web_search_call.action.sources",
            "reasoning.encrypted_content"
        ])
    );
    assert!(body.get("previous_response_id").is_none());
}

#[test]
fn filters_unreplayable_history() {
    let original = json!({"input":[
        {"type":"reasoning","id":"legacy","summary":[{"type":"summary_text","text":"Old summary"}]},
        {"type":"reasoning","id":"null","summary":[],"encrypted_content":null},
        {"type":"reasoning","id":"empty","summary":[],"encrypted_content":""},
        {"type":"reasoning","id":"opaque","summary":[],"encrypted_content":"sealed +/ opaque"},
        {"type":"function_call","name":"inspect","call_id":"call","arguments":"{}"},
        {"role":"assistant","content":"Visible answer"}
    ],"store":true,"previous_response_id":"stored"});
    let mut body = original.clone();
    stateless(&mut body, false).unwrap();
    assert_eq!(
        body["input"],
        json!([
            {"type":"reasoning","id":"opaque","summary":[],"encrypted_content":"sealed +/ opaque"},
            {"type":"function_call","name":"inspect","call_id":"call","arguments":"{}"},
            {"role":"assistant","content":"Visible answer"}
        ])
    );
    assert_eq!(original["input"].as_array().unwrap().len(), 6);
    assert_eq!(body["store"], false);
    assert!(body.get("include").is_none());
    assert!(body.get("previous_response_id").is_none());
}
