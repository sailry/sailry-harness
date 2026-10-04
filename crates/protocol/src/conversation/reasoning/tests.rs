use super::*;

#[test]
fn distinguishes_budget_and_default() {
    for (value, wire) in [
        (Effort::Default, serde_json::json!("default")),
        (Effort::Disabled, serde_json::json!("none")),
        (Effort::XHigh, serde_json::json!("xhigh")),
        (Effort::Budget(-1), serde_json::json!({"budget":-1})),
        (Effort::Budget(4096), serde_json::json!({"budget":4096})),
    ] {
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
        assert_eq!(serde_json::from_value::<Effort>(wire).unwrap(), value);
    }
    for wire in [
        serde_json::json!("unknown"),
        serde_json::json!("4096"),
        serde_json::json!({"budget":4096.5}),
        serde_json::json!({"budget":2147483648_u64}),
    ] {
        assert!(serde_json::from_value::<Effort>(wire).is_err());
    }
}

#[test]
fn enforces_native_bounds_and_choices() {
    for (api, effort, output, valid) in [
        (ModelApi::Anthropic, Effort::Budget(1023), 4096, false),
        (ModelApi::Anthropic, Effort::Budget(1024), 1024, false),
        (ModelApi::Anthropic, Effort::Budget(1024), 1025, true),
        (ModelApi::Anthropic, Effort::Budget(-1), 4096, false),
        (ModelApi::Anthropic, Effort::Disabled, 4096, false),
        (ModelApi::Gemini, Effort::Budget(-2), 4096, false),
        (ModelApi::Gemini, Effort::Budget(-1), 4096, true),
        (ModelApi::Gemini, Effort::Budget(0), 4096, false),
        (ModelApi::Gemini, Effort::Disabled, 4096, true),
        (ModelApi::Gemini, Effort::XHigh, 4096, false),
        (ModelApi::ChatCompletions, Effort::Budget(1024), 4096, false),
        (ModelApi::Responses, Effort::Minimal, 4096, true),
        (ModelApi::DeepSeek, Effort::High, 4096, true),
        (ModelApi::DeepSeek, Effort::Medium, 4096, false),
        (ModelApi::OpenCodeZen, Effort::High, 4096, true),
        (ModelApi::OpenCodeZen, Effort::Budget(-1), 4096, true),
        (ModelApi::OpenCodeZen, Effort::Budget(0), 4096, false),
        (ModelApi::OpenCodeGo, Effort::High, 4096, true),
        (ModelApi::OpenCodeGo, Effort::Budget(1024), 4096, true),
        (ModelApi::OpenCodeGo, Effort::Budget(4096), 4096, false),
        (ModelApi::AzureOpenAi, Effort::High, 4096, true),
        (ModelApi::AzureAi, Effort::Default, 4096, true),
        (ModelApi::AzureAi, Effort::High, 4096, false),
        (ModelApi::Bedrock, Effort::High, 4096, false),
        (ModelApi::Bedrock, Effort::Default, 4096, true),
        (ModelApi::Vertex, Effort::Budget(2048), 4096, true),
    ] {
        assert_eq!(
            effort.validate(api, output).is_ok(),
            valid,
            "{api:?} {effort:?} {output}"
        );
    }
    let mut model = Model {
        id: "fixture".into(),
        context: 8192,
        output: 4096,
        vision: false,
        tools: false,
        reasoning: true,
        web_search: false,
        generates: vec![],
        efforts: vec![Effort::Default, Effort::Budget(1024)],
        custom_efforts: false,
        default_effort: Effort::Budget(1024),
    };
    assert!(model.validate_reasoning(ModelApi::Anthropic).is_ok());
    assert!(
        model
            .validate_effort(ModelApi::Anthropic, Effort::Budget(2048))
            .is_err()
    );
    model.efforts.push(Effort::Default);
    assert!(model.validate_reasoning(ModelApi::Anthropic).is_err());
    model.efforts.pop();
    model.default_effort = Effort::High;
    assert!(model.validate_reasoning(ModelApi::Anthropic).is_err());
    model.efforts.clear();
    assert!(model.validate_reasoning(ModelApi::Anthropic).is_err());
    model.default_effort = Effort::Default;
    assert!(model.validate_reasoning(ModelApi::Anthropic).is_ok());
    assert!(
        model
            .validate_effort(ModelApi::Anthropic, Effort::Default)
            .is_ok()
    );
    assert!(
        model
            .validate_effort(ModelApi::Anthropic, Effort::High)
            .is_err()
    );
}

#[test]
fn selects_first_explicit_choice() {
    assert_eq!(Effort::initial(&[], Effort::High), Effort::Default);
    assert_eq!(
        Effort::initial(
            &[Effort::Default, Effort::Low, Effort::High],
            Effort::Default
        ),
        Effort::Low
    );
    assert_eq!(
        Effort::initial(&[Effort::Low, Effort::High], Effort::High),
        Effort::High
    );
}
