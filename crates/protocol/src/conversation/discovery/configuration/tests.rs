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
fn enables_chatgpt_local_tools() {
    let discovered = model();
    let configured = discovered.configuration(None, ModelApi::Responses, Authentication::ChatGpt);
    assert!(configured.tools);
    assert_eq!(configured.id, "gpt-6.1-sol");
    assert_eq!(configured.display_name.as_deref(), Some("GPT-6.1 Sol"));
    assert!(discovered.capabilities.is_none());
    assert!(!configured.vision);
    assert!(!configured.web_search);
    assert!(!configured.reasoning);
}

#[test]
fn preserves_explicit_tool_capabilities() {
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
            .tools
    );
    reference.tools = Some(true);
    assert!(
        discovered
            .configuration(
                Some(&reference),
                ModelApi::Responses,
                Authentication::ChatGpt
            )
            .tools
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
            .tools
    );
}

#[test]
fn keeps_other_provider_defaults() {
    for authentication in [
        Authentication::ApiKey,
        Authentication::Copilot,
        Authentication::Host,
    ] {
        assert!(
            !model()
                .configuration(None, ModelApi::Responses, authentication)
                .tools
        );
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

#[test]
fn resolves_each_limit_by_source() {
    for authentication in [
        Authentication::ApiKey,
        Authentication::ChatGpt,
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
            let configured =
                discovered.configuration(Some(&reference), ModelApi::Responses, authentication);
            assert_eq!((configured.context, configured.output), expected);
            assert_eq!((discovered.context, discovered.output), native);
        }
    }
}

#[test]
fn completes_only_missing_capabilities() {
    let reference = reference();
    for authentication in [Authentication::ApiKey, Authentication::ChatGpt] {
        let mut discovered = model();
        let configured =
            discovered.configuration(Some(&reference), ModelApi::Responses, authentication);
        assert!(configured.vision && configured.tools && configured.reasoning);
        assert_eq!(configured.generates, [Generation::Image]);
        assert_eq!(configured.efforts, [Effort::Low, Effort::High]);
        assert_eq!(configured.default_effort, Effort::Low);
        assert!(!configured.web_search);
        assert_eq!(configured.display_name, discovered.display_name);

        discovered.capabilities = Some(Capabilities {
            vision: Some(false),
            tools: Some(false),
            reasoning: Some(false),
            efforts: Some(vec![]),
            ..Default::default()
        });
        let configured =
            discovered.configuration(Some(&reference), ModelApi::Responses, authentication);
        assert!(!configured.vision && !configured.tools && !configured.reasoning);
        assert!(configured.efforts.is_empty());
        assert_eq!(configured.default_effort, Effort::Default);

        discovered.capabilities = Some(Capabilities {
            reasoning: Some(true),
            efforts: Some(vec![Effort::High, Effort::Max]),
            default_effort: Some(Effort::Max),
            ..Default::default()
        });
        let configured =
            discovered.configuration(Some(&reference), ModelApi::Responses, authentication);
        assert_eq!(configured.efforts, [Effort::High, Effort::Max]);
        assert_eq!(configured.default_effort, Effort::Max);
    }
}
