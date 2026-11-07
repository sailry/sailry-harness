use super::{Server, provider, response};
use crate::agent::model::{Config, Message, factory, registry};
use futures::TryStreamExt;
use reqwest::header::HeaderMap;
use sailry_protocol::{
    Authentication, Effort, SessionConfig, SessionId,
    conversation::{ModelApi, Provider},
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

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

async fn wire(provider: &Provider, config: &Config, message: Message) -> Value {
    let reply = match provider.api {
        ModelApi::Responses => response(false),
        ModelApi::ChatCompletions | ModelApi::DeepSeek => {
            let event = json!({
                "id":"answer","object":"chat.completion.chunk","created":1,
                "model":config.model_name,
                "choices":[{"index":0,"delta":{"content":"Done"},"finish_reason":"stop"}]
            });
            format!("data: {event}\n\ndata: [DONE]\n\n")
        }
        ModelApi::Gemini => {
            let event = json!({
                "candidates":[{"content":{"role":"model","parts":[{"text":"Done"}]},
                    "finishReason":"STOP","index":0}],
                "modelVersion":config.model_name
            });
            format!("data: {event}\n\n")
        }
        ModelApi::Anthropic => [
            json!({"type":"message_start","message":{"id":"answer","type":"message",
                "role":"assistant","model":config.model_name,"content":[],
                "stop_reason":null,"stop_sequence":null,
                "usage":{"input_tokens":1,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,
                "content_block":{"type":"text","text":"Done"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn",
                "stop_sequence":null},"usage":{"output_tokens":1}}),
            json!({"type":"message_stop"}),
        ]
        .into_iter()
        .map(|event| {
            format!(
                "event: {}\ndata: {event}\n\n",
                event["type"].as_str().unwrap()
            )
        })
        .collect(),
        _ => unreachable!(),
    };
    let server = Server::start(vec![reply]).await;
    let mut provider = provider.clone();
    provider.endpoint = server.endpoint.clone();
    let model = factory::native(
        &registry::Registry::default(),
        &provider,
        &config.model_name,
        SessionId::new(),
        "captured-key",
        HeaderMap::new(),
    )
    .unwrap();
    model
        .call(
            config.request(String::new(), vec![message], Vec::new()),
            CancellationToken::new(),
        )
        .await
        .unwrap()
        .try_collect::<Vec<_>>()
        .await
        .unwrap();
    let mut requests = server.finish().await;
    assert_eq!(requests.len(), 1);
    requests.pop().unwrap().body
}

#[tokio::test]
async fn preserves_compatible_reasoning() {
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
            let config = factory::config(
                &provider,
                &session(&provider, model, effort),
                SessionId::new(),
            );
            let payload = wire(&provider, &config, Message::user().with_text("Hello")).await;
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

#[tokio::test]
async fn preserves_native_effort_levels() {
    let mut provider = provider(ModelApi::Anthropic, "https://example.test".into());
    provider.models = vec![sailry_protocol::conversation::Model {
        output: 16000,
        ..metadata("claude-opus-4-8")
    }];
    let selected = session(&provider, "claude-opus-4-8", Effort::XHigh);
    let config = factory::config(&provider, &selected, SessionId::new());
    assert_eq!(config.request_params["output_config"]["effort"], "xhigh");
    let payload = wire(&provider, &config, Message::user().with_text("Hello")).await;
    assert_eq!(payload["output_config"]["effort"], "xhigh");

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
        let config = factory::config(&provider, &selected, SessionId::new());
        assert!(!config.request_params.contains_key("thinking_level"));
        let payload = wire(&provider, &config, Message::user().with_text("Hello")).await;
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
                    options: ["fast", "ultrafast"]
                        .map(
                            |id| sailry_protocol::conversation::features::FeatureOption {
                                id: id.into(),
                                name: None,
                                icon: None,
                            },
                        )
                        .into(),
                }],
                ..Default::default()
            });
            provider.models = vec![model];
            let mut selected = session(&provider, "captured", Effort::Default);
            let sends = api == ModelApi::Responses && authentication != Authentication::Copilot;
            for (choice, tier) in [("ultrafast", "ultrafast"), ("fast", "priority")] {
                selected.features.insert("speed".into(), choice.into());
                assert_eq!(
                    factory::config(&provider, &selected, SessionId::new())
                        .request_params
                        .get("service_tier")
                        .and_then(Value::as_str),
                    sends.then_some(tier),
                    "{choice}"
                );
            }
            let config = factory::config(&provider, &selected, SessionId::new());
            assert_eq!(
                config
                    .request_params
                    .get("prompt_cache_key")
                    .and_then(Value::as_str)
                    .is_some(),
                authentication == Authentication::ApiKey
                    && matches!(api, ModelApi::Responses | ModelApi::ChatCompletions)
            );
            provider.models[0].capabilities = None;
            assert!(
                factory::config(&provider, &selected, SessionId::new())
                    .request_params
                    .get("service_tier")
                    .is_none()
            );
        }
    }
}

#[tokio::test]
async fn sends_images_without_model_metadata() {
    let mut provider = provider(ModelApi::Responses, "https://example.test/v1".into());
    let selected = session(&provider, "uncatalogued", Effort::Default);
    let config = factory::config(&provider, &selected, SessionId::new());
    let payload = wire(
        &provider,
        &config,
        Message::user().with_image("aW1hZ2U=", "image/png"),
    )
    .await;
    assert!(
        payload["input"]
            .to_string()
            .contains("data:image/png;base64,aW1hZ2U=")
    );
    provider.models = vec![metadata("uncatalogued")];
    assert_eq!(
        factory::config(&provider, &selected, SessionId::new()).supports_vision,
        Some(false)
    );
}
