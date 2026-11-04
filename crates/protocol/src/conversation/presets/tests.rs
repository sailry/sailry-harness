use super::*;
use serde_json::json;

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "keeps_configuration_identity_stable",
            (|scenario: &'static str| {
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
                    reference,
                    "{scenario}"
                );
                assert_eq!(
                    encoded["source"],
                    json!({"kind":"package","data":{"name":"example"}}),
                    "{scenario}"
                );
                assert!(encoded.get("digest").is_none(), "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "preserves_protocol_mapping_and_default_authentication",
            (|scenario: &'static str| {
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
                        protocol,
                        "{scenario}"
                    );
                    assert_eq!(protocol.api(), api, "{scenario}");
                }
                let preset:Preset=serde_json::from_value(json!({"id":"example","name":"Example","routes":[{"protocol":"openai-responses","endpoint":"http://localhost:11434/v1"}]})).unwrap();
                assert_eq!(
                    preset.auth,
                    Auth::ApiKey {
                        header: None,
                        prefix: None
                    },
                    "{scenario}"
                );
                assert_eq!(preset.models, Models::Discover, "{scenario}");
                assert_eq!(preset.routes[0].region, None, "{scenario}");
                assert!(
                    serde_json::to_value(&preset)
                        .unwrap()
                        .get("capabilities")
                        .is_none(),
                    "{scenario}"
                );
                assert!(
                    serde_json::to_value(Binding::default())
                        .unwrap()
                        .get("capabilities")
                        .is_none(),
                    "{scenario}"
                );
            }),
        ),
        (
            "retains_optional_product_display",
            (|scenario: &'static str| {
                let original = json!({"kind":"coding","default_model":"fixture"});
                let product: Product = serde_json::from_value(original.clone()).unwrap();
                assert!(product.display.is_none(), "{scenario}");
                let display = json!({"label":"Example Plan","locales":{"zh-CN":"示例套餐"}});
                let mut declared = original;
                declared["display"] = display.clone();
                let product: Product = serde_json::from_value(declared).unwrap();
                assert_eq!(
                    product.display.as_ref().unwrap().label("zh-CN"),
                    "示例套餐",
                    "{scenario}"
                );
                assert_eq!(
                    product.display.as_ref().unwrap().label("en"),
                    "Example Plan",
                    "{scenario}"
                );
                let encoded = serde_json::to_value(&product).unwrap();
                assert_eq!(encoded["display"]["label"], display["label"], "{scenario}");
                assert_eq!(
                    encoded["display"]["locales"], display["locales"],
                    "{scenario}"
                );
                assert_eq!(
                    serde_json::from_value::<Product>(encoded).unwrap(),
                    product,
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
