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
