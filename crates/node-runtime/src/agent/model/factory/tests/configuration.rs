use super::*;

fn metadata(id: &str) -> sailry_protocol::conversation::Model {
    sailry_protocol::conversation::Model {
        id: id.into(),
        display_name: None,
        context: 100_000,
        output: 4096,
        compaction_tokens: None,
        capabilities: None,
        vision: false,
        tools: true,
        reasoning: true,
        generates: vec![],
        efforts: vec![],
        custom_efforts: false,
        default_effort: Effort::High,
    }
}

fn session(provider: &Provider, model: &str, effort: Effort) -> SessionConfig {
    serde_json::from_value(json!({
        "provider":provider.id,"model":model,"effort":effort,"mode":"code",
        "permission":"ask","credential":null
    }))
    .unwrap()
}

#[test]
fn preserves_compatible_reasoning() {
    for (api, model, choices) in [
        (
            ModelApi::ChatCompletions,
            "glm-5",
            vec![
                (Effort::Disabled, "none"),
                (Effort::Minimal, "minimal"),
                (Effort::XHigh, "xhigh"),
            ],
        ),
        (
            ModelApi::DeepSeek,
            "deepseek-v4-pro",
            vec![
                (Effort::Disabled, ""),
                (Effort::High, "high"),
                (Effort::Max, "max"),
            ],
        ),
    ] {
        let mut provider = provider(api, "https://example.test/v1".into());
        provider.models = vec![metadata(model)];
        for (effort, expected) in choices {
            let config = config(
                &provider,
                &session(&provider, model, effort),
                SessionId::new(),
            );
            let payload = goose_provider_types::formats::openai::create_request(
                &config,
                "",
                &[],
                &[],
                &goose_provider_types::images::ImageFormat::OpenAi,
                true,
            )
            .unwrap();
            if expected.is_empty() {
                assert!(payload.get("reasoning_effort").is_none());
            } else {
                assert_eq!(payload["reasoning_effort"], expected);
            }
            assert_eq!(
                payload["thinking"]["type"],
                if effort == Effort::Disabled {
                    "disabled"
                } else {
                    "enabled"
                }
            );
            if api == ModelApi::ChatCompletions {
                assert_eq!(payload["thinking"]["clear_thinking"], false);
            }
        }
    }
}

#[test]
fn preserves_native_effort_levels() {
    let mut provider = provider(ModelApi::Anthropic, "https://example.test".into());
    provider.models = vec![sailry_protocol::conversation::Model {
        output: 16000,
        ..metadata("claude-opus-4-8")
    }];
    let selected = session(&provider, "claude-opus-4-8", Effort::XHigh);
    let config = config(&provider, &selected, SessionId::new());
    assert_eq!(
        config.request_param::<Value>("output_config").unwrap()["effort"],
        "xhigh"
    );

    provider.api = ModelApi::Gemini;
    for (model, effort, expected) in [
        (
            "gemini-3-flash-preview",
            Effort::Low,
            json!({"thinkingLevel":"low","includeThoughts":true}),
        ),
        (
            "gemini-3-flash-preview",
            Effort::Medium,
            json!({"thinkingLevel":"medium","includeThoughts":true}),
        ),
        (
            "gemini-3-pro-preview",
            Effort::Medium,
            json!({"thinkingLevel":"low","includeThoughts":true}),
        ),
        (
            "gemini-3-flash-preview",
            Effort::High,
            json!({"thinkingLevel":"high","includeThoughts":true}),
        ),
        (
            "gemini-2.5-flash",
            Effort::Budget(2048),
            json!({"thinkingBudget":2048,"includeThoughts":true}),
        ),
        (
            "gemini-2.5-flash",
            Effort::Disabled,
            json!({"thinkingBudget":0,"includeThoughts":false}),
        ),
        ("custom-gemini", Effort::Budget(2048), Value::Null),
    ] {
        provider.models = vec![metadata(model)];
        let selected = session(&provider, model, effort);
        let config = super::config(&provider, &selected, SessionId::new());
        assert!(config.request_param::<Value>("thinking_level").is_none());
        let payload =
            goose_provider_types::formats::google::create_request(&config, "", &[], &[]).unwrap();
        assert_eq!(
            payload["generationConfig"]["thinkingConfig"], expected,
            "{model}, {effort:?}"
        );
    }
}

#[test]
fn scopes_optional_request_fields() {
    for api in [
        ModelApi::Responses,
        ModelApi::ChatCompletions,
        ModelApi::Anthropic,
        ModelApi::Plugin,
    ] {
        for authentication in [
            Authentication::ApiKey,
            Authentication::Copilot,
            Authentication::Plugin,
        ] {
            let mut provider = provider(api, "https://example.test/v1".into());
            provider.authentication = authentication;
            let mut model = metadata("captured");
            model.capabilities = Some(sailry_protocol::conversation::discovery::Capabilities {
                features: vec![sailry_protocol::conversation::features::Feature {
                    id: "speed".into(),
                    name: None,
                    icon: None,
                    options: vec![sailry_protocol::conversation::features::FeatureOption {
                        id: "fast".into(),
                        name: None,
                    }],
                }],
                ..Default::default()
            });
            provider.models = vec![model];
            let mut selected = session(&provider, "captured", Effort::Default);
            selected.features.insert("speed".into(), "fast".into());
            let config = config(&provider, &selected, SessionId::new());
            assert_eq!(
                config.request_param::<String>("service_tier").is_some(),
                api == ModelApi::Responses && authentication != Authentication::Copilot
            );
            assert_eq!(
                config.request_param::<String>("prompt_cache_key").is_some(),
                authentication == Authentication::ApiKey
                    && matches!(api, ModelApi::Responses | ModelApi::ChatCompletions)
            );
            provider.models[0].capabilities = None;
            assert!(
                super::config(&provider, &selected, SessionId::new())
                    .request_param::<String>("service_tier")
                    .is_none()
            );
        }
    }
}

#[test]
fn sends_images_without_model_metadata() {
    let mut provider = provider(ModelApi::Responses, "https://example.test/v1".into());
    let selected = session(&provider, "uncatalogued", Effort::Default);
    let config = config(&provider, &selected, SessionId::new());
    let payload = goose_provider_types::formats::openai_responses::create_responses_request(
        &config,
        "",
        &[Message::user().with_image("aW1hZ2U=", "image/png")],
        &[],
    )
    .unwrap();
    assert!(
        payload["input"]
            .to_string()
            .contains("data:image/png;base64,aW1hZ2U=")
    );
    provider.models = vec![metadata("uncatalogued")];
    assert_eq!(
        super::config(&provider, &selected, SessionId::new()).supports_vision,
        Some(false)
    );
}
