use super::*;

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
        provider.models = serde_json::from_value(json!([{
            "id":model,"reasoning":true,"default_effort":"high"
        }]))
        .unwrap();
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
    provider.models = serde_json::from_value(json!([{
        "id":"claude-opus-4-8","reasoning":true,"output":16000
    }]))
    .unwrap();
    let selected = session(&provider, "claude-opus-4-8", Effort::XHigh);
    let config = config(&provider, &selected, SessionId::new());
    assert_eq!(
        config.request_param::<Value>("output_config").unwrap()["effort"],
        "xhigh"
    );

    provider.api = ModelApi::Gemini;
    let selected = session(&provider, "gemini-3-flash-preview", Effort::Minimal);
    let config = super::config(&provider, &selected, SessionId::new());
    let payload =
        goose_provider_types::formats::google::create_request(&config, "", &[], &[]).unwrap();
    assert_eq!(
        payload["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "minimal"
    );
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
            provider.models = serde_json::from_value(json!([{
                "id":"captured","capabilities":{"features":[{"id":"speed","options":[{"id":"fast"}]}]}
            }])).unwrap();
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
    provider.models =
        serde_json::from_value(json!([{"id":"uncatalogued","vision":false}])).unwrap();
    assert_eq!(
        super::config(&provider, &selected, SessionId::new()).supports_vision,
        Some(false)
    );
}
