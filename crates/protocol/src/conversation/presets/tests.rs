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

#[test]
fn retains_optional_product_display() {
    let original = json!({"kind":"coding","default_model":"fixture"});
    let product: Product = serde_json::from_value(original.clone()).unwrap();
    assert!(product.display.is_none());
    let display = json!({"label":"Example Plan","locales":{"zh-CN":"示例套餐"}});
    let mut declared = original;
    declared["display"] = display.clone();
    let product: Product = serde_json::from_value(declared).unwrap();
    assert_eq!(product.display.as_ref().unwrap().label("zh-CN"), "示例套餐");
    assert_eq!(
        product.display.as_ref().unwrap().label("en"),
        "Example Plan"
    );
    let encoded = serde_json::to_value(&product).unwrap();
    assert_eq!(encoded["display"]["label"], display["label"]);
    assert_eq!(encoded["display"]["locales"], display["locales"]);
    assert_eq!(serde_json::from_value::<Product>(encoded).unwrap(), product);
}
