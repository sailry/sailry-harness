use super::*;
use serde_json::json;

#[test]
fn keeps_configuration_identity_stable() {
    let reference = PresetRef {
        source: Source::Package {
            name: "example".into(),
        },
        id: "channel".into(),
        route: Some(RouteKey {
            protocol: Protocol::OpenAiChatCompletions,
            region: Some(Region::Global),
            product: Some(ProductKind::Coding),
        }),
    };
    let encoded = serde_json::to_value(&reference).unwrap();
    assert_eq!(
        serde_json::from_value::<PresetRef>(encoded.clone()).unwrap(),
        reference
    );
    assert_eq!(
        encoded["source"],
        json!({"kind":"package","data":{"name":"example"}})
    );
    assert!(encoded.get("digest").is_none());
}

#[test]
fn preserves_protocol_mapping_and_default_authentication() {
    for (name, protocol, api) in [
        (
            "openai-responses",
            Protocol::OpenAiResponses,
            ModelApi::Responses,
        ),
        (
            "openai-chat-completions",
            Protocol::OpenAiChatCompletions,
            ModelApi::ChatCompletions,
        ),
        ("anthropic", Protocol::Anthropic, ModelApi::Anthropic),
        ("gemini", Protocol::Gemini, ModelApi::Gemini),
        ("deepseek", Protocol::DeepSeek, ModelApi::DeepSeek),
        ("opencode-go", Protocol::OpenCodeGo, ModelApi::OpenCodeGo),
        ("opencode-zen", Protocol::OpenCodeZen, ModelApi::OpenCodeZen),
        ("azure-openai", Protocol::AzureOpenAi, ModelApi::AzureOpenAi),
        ("azure-ai", Protocol::AzureAi, ModelApi::AzureAi),
    ] {
        assert_eq!(
            serde_json::from_value::<Protocol>(json!(name)).unwrap(),
            protocol
        );
        assert_eq!(protocol.api(), api);
    }
    let preset:Preset=serde_json::from_value(json!({"id":"example","name":"Example","routes":[{"protocol":"openai-responses","endpoint":"http://localhost:11434/v1"}]})).unwrap();
    assert_eq!(
        preset.auth,
        Auth::ApiKey {
            header: None,
            prefix: None
        }
    );
    assert_eq!(preset.models, Models::Discover);
    assert_eq!(preset.routes[0].region, None);
}
