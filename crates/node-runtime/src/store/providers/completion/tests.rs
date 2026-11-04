use super::*;
use sailry_protocol::{Authentication, conversation::ModelApi};

fn model() -> Model {
    serde_json::from_value(serde_json::json!({
        "id": "custom-model",
        "context": 8192,
        "output": 2048,
        "vision": false,
        "tools": false,
        "reasoning": true,
    }))
    .unwrap()
}

fn reference() -> catalog::Model {
    catalog::Model {
        id: "custom-model".into(),
        name: "Reference".into(),
        context: Some(1_000_000),
        output: Some(128_000),
        inputs: vec!["text".into(), "image".into()],
        outputs: vec!["text".into(), "image".into()],
        tools: Some(true),
        reasoning: Some(true),
        options: vec![catalog::Reasoning::Effort {
            values: vec![Some("low".into()), Some("high".into())],
        }],
    }
}

#[test]
fn preserves_storage_contracts() {
    for (scenario, check) in [
        (
            "attempts_unknown_features",
            (|scenario: &'static str| {
                for api in [
                    ModelApi::ChatCompletions,
                    ModelApi::Responses,
                    ModelApi::Anthropic,
                    ModelApi::Gemini,
                    ModelApi::DeepSeek,
                    ModelApi::OpenCodeGo,
                    ModelApi::OpenCodeZen,
                    ModelApi::AzureOpenAi,
                    ModelApi::AzureAi,
                ] {
                    let mut model = model();
                    resolve(&mut model, None, api, Authentication::ApiKey);
                    assert!(model.tools && model.vision, "{scenario}");
                    assert!(!model.reasoning, "{scenario}");
                    assert!(model.efforts.is_empty(), "{scenario}");
                    assert_eq!(model.default_effort, Effort::Default, "{scenario}");
                    assert!(model.capabilities.is_none(), "{scenario}");
                    assert_eq!((model.context, model.output), (8192, 2048), "{scenario}");
                }
            }) as fn(&'static str),
        ),
        (
            "clears_unverified_generation_flags",
            (|scenario: &'static str| {
                let mut model = model();
                model.generates = vec![sailry_protocol::media::Generation::Image];
                resolve(
                    &mut model,
                    None,
                    ModelApi::Responses,
                    Authentication::ApiKey,
                );
                assert!(model.generates.is_empty(), "{scenario}");
            }),
        ),
        (
            "native_metadata_precedes_cached",
            (|scenario: &'static str| {
                let mut model = model();
                model.capabilities = Some(discovery::Capabilities {
                    vision: Some(false),
                    tools: Some(false),
                    reasoning: Some(false),
                    efforts: Some(vec![]),
                    ..Default::default()
                });
                let native = model.capabilities.clone();
                resolve(
                    &mut model,
                    Some(&reference()),
                    ModelApi::Responses,
                    Authentication::ChatGptOAuth,
                );
                assert!(
                    !model.vision && !model.tools && !model.reasoning,
                    "{scenario}"
                );
                assert_eq!(model.capabilities, native, "{scenario}");
                assert_eq!((model.context, model.output), (8192, 2048), "{scenario}");
            }),
        ),
        (
            "cached_metadata_completes_summaries",
            (|scenario: &'static str| {
                let mut model = model();
                let mut reference = reference();
                reference.inputs = vec!["text".into()];
                reference.tools = Some(false);
                resolve(
                    &mut model,
                    Some(&reference),
                    ModelApi::Responses,
                    Authentication::ApiKey,
                );
                assert!(!model.vision && !model.tools, "{scenario}");
                assert!(model.reasoning, "{scenario}");
                assert_eq!(
                    model.generates,
                    [sailry_protocol::media::Generation::Image],
                    "{scenario}"
                );
                assert_eq!(model.efforts, [Effort::Low, Effort::High], "{scenario}");
                assert_eq!(model.default_effort, Effort::Low, "{scenario}");
                assert!(model.capabilities.is_none(), "{scenario}");
            }),
        ),
        (
            "preserves_explicit_choices_and_limits",
            (|scenario: &'static str| {
                let mut model = model();
                model.compaction_tokens = Some(200_000);
                model.efforts = vec![Effort::High];
                model.default_effort = Effort::High;
                model.custom_efforts = true;
                resolve(
                    &mut model,
                    Some(&reference()),
                    ModelApi::Responses,
                    Authentication::ApiKey,
                );
                assert_eq!(model.efforts, [Effort::High], "{scenario}");
                assert_eq!(model.default_effort, Effort::High, "{scenario}");
                assert!(model.custom_efforts, "{scenario}");
                assert_eq!(model.compaction_tokens, Some(200_000), "{scenario}");
                assert_eq!((model.context, model.output), (8192, 2048), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
