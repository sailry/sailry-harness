use super::*;
use crate::conversation::discovery::Capabilities;

fn model() -> Model {
    Model {
        id: "gpt-6.1-sol".into(),
        display_name: Some("GPT-6.1 Sol".into()),
        context: None,
        output: None,
        capabilities: None,
    }
}

#[test]
fn preserves_wire_and_validation() {
    for (scenario, check) in [
        (
            "attempts_unknown_input_and_tools",
            (|scenario: &'static str| {
                let discovered = model();
                for api in [
                    ModelApi::Responses,
                    ModelApi::ChatCompletions,
                    ModelApi::Anthropic,
                    ModelApi::Gemini,
                    ModelApi::DeepSeek,
                    ModelApi::OpenCodeGo,
                    ModelApi::OpenCodeZen,
                    ModelApi::AzureOpenAi,
                    ModelApi::AzureAi,
                ] {
                    for authentication in [
                        Authentication::ApiKey,
                        Authentication::ChatGpt,
                        Authentication::ChatGptOAuth,
                        Authentication::Copilot,
                    ] {
                        let configured = discovered.configuration(None, api, authentication);
                        assert!(configured.tools && configured.vision, "{scenario}");
                        assert_eq!(configured.id, "gpt-6.1-sol", "{scenario}");
                        assert_eq!(
                            configured.display_name.as_deref(),
                            Some("GPT-6.1 Sol"),
                            "{scenario}"
                        );
                        assert!(discovered.capabilities.is_none(), "{scenario}");
                        assert!(!configured.reasoning, "{scenario}");
                        assert!(configured.efforts.is_empty(), "{scenario}");
                        assert_eq!(configured.default_effort, Effort::Default, "{scenario}");
                    }
                }
            }) as fn(&'static str),
        ),
        (
            "preserves_explicit_tool_capabilities",
            (|scenario: &'static str| {
                let mut discovered = model();
                let mut reference = catalog::Model {
                    id: discovered.id.clone(),
                    name: "Reference".into(),
                    context: None,
                    output: None,
                    inputs: vec![],
                    outputs: vec![],
                    tools: Some(false),
                    reasoning: None,
                    options: vec![],
                };
                assert!(
                    !discovered
                        .configuration(
                            Some(&reference),
                            ModelApi::Responses,
                            Authentication::ChatGpt
                        )
                        .tools,
                    "{scenario}"
                );
                reference.tools = Some(true);
                assert!(
                    discovered
                        .configuration(
                            Some(&reference),
                            ModelApi::Responses,
                            Authentication::ChatGpt
                        )
                        .tools,
                    "{scenario}"
                );
                discovered.capabilities = Some(Capabilities {
                    tools: Some(false),
                    ..Default::default()
                });
                assert!(
                    !discovered
                        .configuration(
                            Some(&reference),
                            ModelApi::Responses,
                            Authentication::ChatGpt
                        )
                        .tools,
                    "{scenario}"
                );
            }),
        ),
        (
            "respects_text_only_input_metadata",
            (|scenario: &'static str| {
                let mut reference = reference();
                reference.inputs = vec!["text".into()];
                let configured = model().configuration(
                    Some(&reference),
                    ModelApi::Responses,
                    Authentication::ApiKey,
                );
                assert!(!configured.vision, "{scenario}");
            }),
        ),
        (
            "resolves_each_limit_by_source",
            (|scenario: &'static str| {
                for authentication in [
                    Authentication::ApiKey,
                    Authentication::ChatGpt,
                    Authentication::ChatGptOAuth,
                    Authentication::Copilot,
                ] {
                    for (native, cached, expected) in [
                        ((None, None), (None, None), (200_000, 16_384)),
                        ((Some(8192), None), (None, None), (8192, 8192)),
                        ((None, Some(256_000)), (None, None), (256_000, 256_000)),
                        (
                            (None, None),
                            (Some(1_050_000), Some(128_000)),
                            (1_050_000, 128_000),
                        ),
                        (
                            (Some(272_000), None),
                            (Some(1_050_000), Some(128_000)),
                            (272_000, 128_000),
                        ),
                        (
                            (None, Some(64_000)),
                            (Some(1_050_000), Some(128_000)),
                            (1_050_000, 64_000),
                        ),
                        (
                            (Some(272_000), Some(64_000)),
                            (Some(1_050_000), Some(128_000)),
                            (272_000, 64_000),
                        ),
                        ((None, None), (Some(1_050_000), None), (1_050_000, 16_384)),
                        ((None, None), (None, Some(64_000)), (200_000, 64_000)),
                    ] {
                        let mut discovered = model();
                        (discovered.context, discovered.output) = native;
                        let mut reference = reference();
                        (reference.context, reference.output) = cached;
                        let configured = discovered.configuration(
                            Some(&reference),
                            ModelApi::Responses,
                            authentication,
                        );
                        assert_eq!(
                            (configured.context, configured.output),
                            expected,
                            "{scenario}"
                        );
                        assert_eq!(
                            (discovered.context, discovered.output),
                            native,
                            "{scenario}"
                        );
                    }
                }
            }),
        ),
        (
            "completes_only_missing_capabilities",
            (|scenario: &'static str| {
                let reference = reference();
                for authentication in [
                    Authentication::ApiKey,
                    Authentication::ChatGpt,
                    Authentication::ChatGptOAuth,
                ] {
                    let mut discovered = model();
                    let configured = discovered.configuration(
                        Some(&reference),
                        ModelApi::Responses,
                        authentication,
                    );
                    assert!(
                        configured.vision && configured.tools && configured.reasoning,
                        "{scenario}"
                    );
                    assert_eq!(configured.generates, [Generation::Image], "{scenario}");
                    assert_eq!(
                        configured.efforts,
                        [Effort::Low, Effort::High],
                        "{scenario}"
                    );
                    assert_eq!(configured.default_effort, Effort::Low, "{scenario}");
                    assert_eq!(
                        configured.display_name, discovered.display_name,
                        "{scenario}"
                    );

                    discovered.capabilities = Some(Capabilities {
                        vision: Some(false),
                        tools: Some(false),
                        reasoning: Some(false),
                        efforts: Some(vec![]),
                        ..Default::default()
                    });
                    let configured = discovered.configuration(
                        Some(&reference),
                        ModelApi::Responses,
                        authentication,
                    );
                    assert!(
                        !configured.vision && !configured.tools && !configured.reasoning,
                        "{scenario}"
                    );
                    assert!(configured.efforts.is_empty(), "{scenario}");
                    assert_eq!(configured.default_effort, Effort::Default, "{scenario}");

                    discovered.capabilities = Some(Capabilities {
                        reasoning: Some(true),
                        efforts: Some(vec![Effort::High, Effort::Max]),
                        default_effort: Some(Effort::Max),
                        ..Default::default()
                    });
                    let configured = discovered.configuration(
                        Some(&reference),
                        ModelApi::Responses,
                        authentication,
                    );
                    assert_eq!(
                        configured.efforts,
                        [Effort::High, Effort::Max],
                        "{scenario}"
                    );
                    assert_eq!(configured.default_effort, Effort::Max, "{scenario}");
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

fn reference() -> catalog::Model {
    catalog::Model {
        id: "gpt-6.1-sol".into(),
        name: "Reference model".into(),
        context: Some(1_050_000),
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
