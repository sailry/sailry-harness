use super::*;

fn input(model: &str, reasoning: Option<bool>, effort: Effort) -> Request {
    Request {
        model: model.into(),
        reasoning,
        effort,
        max_tokens: Some(8192),
        ..Request::default()
    }
}

#[test]
fn encrypted_content_follows_capability() {
    for (model, capability, effort, included) in [
        ("gpt-6-fixture", Some(false), Effort::Default, false),
        ("gpt-4o", None, Effort::Default, false),
        ("gpt-6-fixture", None, Effort::High, true),
        ("unknown-fixture", None, Effort::High, false),
        ("reasoning-fixture", Some(true), Effort::Disabled, true),
        ("deepseek-r1", Some(true), Effort::High, false),
        ("qwen/Qwen3-Thinking", Some(true), Effort::High, false),
        ("z-ai/GLM-5", Some(true), Effort::High, false),
        ("moonshotai/kimi-k2.5", Some(true), Effort::High, false),
        ("claude-sonnet-4-5", Some(true), Effort::High, false),
    ] {
        let mut request = input(model, capability, effort);
        prepare(&mut request, ModelApi::Responses, ModelApi::Responses);
        assert_eq!(request.params.contains_key("include"), included, "{model}");
    }
}

#[test]
fn native_budget_is_consumed_after_translation() {
    for (api, name, field) in [
        (ModelApi::Anthropic, "claude-sonnet-4-5", "budget_tokens"),
        (ModelApi::Gemini, "gemini-2.5-pro", "thinking_budget"),
    ] {
        let mut request = input(name, Some(true), Effort::Budget(2048));
        request.params.insert(field.into(), json!(4096));
        prepare(&mut request, api, api);
        assert!(!request.params.contains_key(field));
        if api == ModelApi::Anthropic {
            assert_eq!(request.params["thinking"]["budget_tokens"], 4096);
        } else {
            assert_eq!(
                request.params["generationConfig"]["thinkingConfig"]["thinkingBudget"],
                4096
            );
        }
    }
}

#[test]
fn unknown_effort_reaches_provider() {
    for api in [ModelApi::ChatCompletions, ModelApi::Responses] {
        let mut request = input("fixture", Some(false), Effort::High);
        request
            .params
            .insert("reasoning_effort".into(), "high".into());
        prepare(&mut request, api, api);
        assert_eq!(request.effort, Effort::Default);
        if api == ModelApi::Responses {
            assert_eq!(
                request.params["reasoning"],
                json!({"effort":"high","summary":"auto"})
            );
            assert!(!request.params.contains_key("reasoning_effort"));
        } else {
            assert_eq!(request.params["reasoning_effort"], "high");
        }
    }
}

#[test]
fn output_field_follows_channel() {
    for (configured, effective, field) in [
        (
            ModelApi::ChatCompletions,
            ModelApi::ChatCompletions,
            "max_completion_tokens",
        ),
        (
            ModelApi::AzureOpenAi,
            ModelApi::ChatCompletions,
            "max_completion_tokens",
        ),
        (ModelApi::AzureAi, ModelApi::ChatCompletions, "max_tokens"),
        (ModelApi::DeepSeek, ModelApi::DeepSeek, "max_tokens"),
    ] {
        let mut request = input("fixture", Some(true), Effort::High);
        prepare(&mut request, effective, configured);
        assert_eq!(request.params[field], 8192);
        assert_eq!(request.max_tokens, None);
        assert_eq!(request.effort, Effort::Default);
    }
}

#[test]
fn budget_preserves_answer_capacity() {
    for (effort, output, expected) in [
        (Effort::Low, 8192, Some(4000)),
        (Effort::Medium, 8192, Some(7168)),
        (Effort::High, 8192, Some(7168)),
        (Effort::XHigh, 8192, Some(7168)),
        (Effort::Max, 8192, Some(7168)),
        (Effort::Budget(1024), 8192, Some(1024)),
        (Effort::Budget(4096), 8192, Some(4096)),
        (Effort::High, 512, None),
    ] {
        let mut request = input("fixture", Some(true), effort);
        request.max_tokens = Some(output);
        prepare(&mut request, ModelApi::Anthropic, ModelApi::Anthropic);
        assert_eq!(
            request
                .params
                .get("thinking")
                .and_then(|thinking| thinking["budget_tokens"].as_u64()),
            expected,
            "{effort:?}"
        );
        assert!(!request.params.contains_key("output_config"));
        assert_eq!(request.effort, Effort::Default);
    }
}

#[test]
fn adaptive_models_keep_selected_effort() {
    let mut request = input("claude-opus-4-8", Some(true), Effort::XHigh);
    prepare(&mut request, ModelApi::Anthropic, ModelApi::Anthropic);
    assert_eq!(
        request.params["thinking"],
        json!({"type":"adaptive","display":"summarized"})
    );
    assert_eq!(request.params["output_config"], json!({"effort":"xhigh"}));
    let mut request = input("claude-opus-4-8", Some(true), Effort::Disabled);
    prepare(&mut request, ModelApi::Anthropic, ModelApi::Anthropic);
    assert_eq!(request.params["thinking"], json!({"type":"disabled"}));
    assert!(!request.params.contains_key("output_config"));
}

#[test]
fn raw_parameters_keep_precedence() {
    let mut request = input("claude-opus-4-8", Some(true), Effort::High);
    request.params = json!({
        "thinking":{"type":"adaptive","display":"omitted"},
        "output_config":{"effort":"xhigh"}
    })
    .as_object()
    .unwrap()
    .clone();
    prepare(&mut request, ModelApi::Anthropic, ModelApi::Anthropic);
    assert_eq!(
        request.params["thinking"],
        json!({"type":"adaptive","display":"omitted"})
    );
    assert_eq!(request.params["output_config"], json!({"effort":"xhigh"}));
}
