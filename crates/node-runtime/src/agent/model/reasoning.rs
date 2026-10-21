use adk_model::{
    anthropic::{AnthropicConfig, Effort as AnthropicEffort, ThinkingMode},
    gemini::{ThinkingConfig, ThinkingLevel},
    openai::OpenAIReasoningEffort,
};
use sailry_protocol::{
    Effort, Fault,
    conversation::{Model, ModelApi},
};

#[derive(Clone, Copy)]
pub(super) struct Resolved {
    pub effort: Effort,
    pub summary: bool,
    pub encrypted: bool,
}

pub(super) fn resolve(
    model: Option<&Model>,
    api: ModelApi,
    selected: Effort,
) -> Result<Resolved, Fault> {
    let known = model
        .and_then(|model| model.capabilities.as_ref())
        .and_then(|value| value.reasoning);
    let effort = if known == Some(false) {
        Effort::Default
    } else if selected == Effort::Default {
        model
            .filter(|model| {
                model.default_effort != Effort::Default
                    && model.validate_effort(api, model.default_effort).is_ok()
            })
            .map_or(selected, |model| model.default_effort)
    } else {
        selected
    };
    effort.validate(api, model.map_or(0, |model| model.output))?;
    Ok(Resolved {
        effort,
        encrypted: known != Some(false)
            && (known == Some(true)
                || model.is_some_and(|model| model.reasoning)
                || !matches!(effort, Effort::Default | Effort::Disabled)),
        summary: effort != Effort::Disabled
            && known != Some(false)
            && (model.is_some_and(|model| model.reasoning) || effort != Effort::Default),
    })
}

fn unsupported() -> Fault {
    Fault::invalid("reasoning choice is unsupported by this API")
}

pub(super) fn deepseek(
    config: adk_model::deepseek::DeepSeekConfig,
    effort: Effort,
) -> Result<adk_model::deepseek::DeepSeekConfig, Fault> {
    use adk_model::deepseek::{ReasoningEffort, ThinkingMode};
    Ok(match effort {
        Effort::Default => config,
        Effort::Disabled => config.with_thinking_mode(ThinkingMode::Disabled),
        Effort::High => config
            .with_thinking_mode(ThinkingMode::Enabled)
            .with_reasoning_effort(ReasoningEffort::High),
        Effort::Max => config
            .with_thinking_mode(ThinkingMode::Enabled)
            .with_reasoning_effort(ReasoningEffort::Max),
        _ => return Err(unsupported()),
    })
}

pub(super) fn openai(effort: Effort) -> Result<Option<OpenAIReasoningEffort>, Fault> {
    Ok(Some(match effort {
        Effort::Default => return Ok(None),
        Effort::Disabled => OpenAIReasoningEffort::None,
        Effort::Minimal => OpenAIReasoningEffort::Minimal,
        Effort::Low => OpenAIReasoningEffort::Low,
        Effort::Medium => OpenAIReasoningEffort::Medium,
        Effort::High => OpenAIReasoningEffort::High,
        Effort::XHigh => OpenAIReasoningEffort::XHigh,
        Effort::Max => OpenAIReasoningEffort::Max,
        Effort::Budget(_) => return Err(unsupported()),
    }))
}

pub(super) fn anthropic(config: AnthropicConfig, effort: Effort) -> Result<AnthropicConfig, Fault> {
    let effort = match effort {
        Effort::Default => return Ok(config),
        Effort::Budget(tokens) => {
            let tokens = u32::try_from(tokens).map_err(|_| unsupported())?;
            return Ok(config.with_thinking_mode(ThinkingMode::Enabled {
                budget_tokens: tokens,
            }));
        }
        Effort::Low => AnthropicEffort::Low,
        Effort::Medium => AnthropicEffort::Medium,
        Effort::High => AnthropicEffort::High,
        Effort::XHigh => AnthropicEffort::XHigh,
        Effort::Max => AnthropicEffort::Max,
        Effort::Disabled | Effort::Minimal => return Err(unsupported()),
    };
    Ok(config
        .with_thinking_mode(ThinkingMode::Adaptive)
        .with_effort(effort))
}

pub(super) fn gemini(effort: Effort) -> Result<Option<ThinkingConfig>, Fault> {
    let config = ThinkingConfig::new().with_thoughts_included(true);
    let level = match effort {
        Effort::Default => return Ok(None),
        Effort::Disabled => {
            return Ok(Some(ThinkingConfig::new().with_thinking_budget(0)));
        }
        Effort::Budget(tokens) => return Ok(Some(config.with_thinking_budget(tokens))),
        Effort::Minimal => ThinkingLevel::Minimal,
        Effort::Low => ThinkingLevel::Low,
        Effort::Medium => ThinkingLevel::Medium,
        Effort::High => ThinkingLevel::High,
        Effort::XHigh | Effort::Max => return Err(unsupported()),
    };
    Ok(Some(config.with_thinking_level(level)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_catalog_and_capability() {
        let mut model: Model = serde_json::from_value(serde_json::json!({
            "id":"fixture", "context":32000, "output":8192,
            "efforts":["low","high"], "default_effort":"high"
        }))
        .unwrap();
        let resolved = resolve(Some(&model), ModelApi::Responses, Effort::Default).unwrap();
        assert_eq!(resolved.effort, Effort::High);
        assert!(resolved.summary);
        assert!(resolved.encrypted);
        model.capabilities =
            Some(serde_json::from_value(serde_json::json!({"reasoning":false})).unwrap());
        let resolved = resolve(Some(&model), ModelApi::Responses, Effort::High).unwrap();
        assert_eq!(resolved.effort, Effort::Default);
        assert!(!resolved.summary);
        assert!(!resolved.encrypted);
        assert!(
            !resolve(None, ModelApi::Responses, Effort::Default)
                .unwrap()
                .encrypted
        );
        assert!(
            !resolve(None, ModelApi::Responses, Effort::Disabled)
                .unwrap()
                .encrypted
        );
        assert!(
            resolve(None, ModelApi::Responses, Effort::High)
                .unwrap()
                .encrypted
        );
        model.reasoning = true;
        model.capabilities = None;
        model.default_effort = Effort::Default;
        assert!(
            resolve(Some(&model), ModelApi::Responses, Effort::Disabled)
                .unwrap()
                .encrypted
        );
        model.reasoning = false;
        model.capabilities =
            Some(serde_json::from_value(serde_json::json!({"reasoning":true})).unwrap());
        assert!(
            resolve(Some(&model), ModelApi::Responses, Effort::Default)
                .unwrap()
                .encrypted
        );
        assert!(
            !resolve(None, ModelApi::Responses, Effort::Default)
                .unwrap()
                .summary
        );
        assert!(
            resolve(None, ModelApi::Responses, Effort::High)
                .unwrap()
                .summary
        );
        assert!(
            !resolve(None, ModelApi::Responses, Effort::Disabled)
                .unwrap()
                .summary
        );
    }

    #[test]
    fn deepseek_limits_effort() {
        use adk_model::deepseek::{DeepSeekConfig, ReasoningEffort, ThinkingMode};
        let config = || DeepSeekConfig::new("fixture", "fixture");
        let default = deepseek(config(), Effort::Default).unwrap();
        assert_eq!(default.thinking, None);
        assert_eq!(default.reasoning_effort, None);
        let high = deepseek(config(), Effort::High).unwrap();
        assert_eq!(high.thinking, Some(ThinkingMode::Enabled));
        assert_eq!(high.reasoning_effort, Some(ReasoningEffort::High));
        let max = deepseek(config(), Effort::Max).unwrap();
        assert_eq!(max.thinking, Some(ThinkingMode::Enabled));
        assert_eq!(max.reasoning_effort, Some(ReasoningEffort::Max));
        assert_eq!(deepseek(config(), Effort::Low).unwrap_err(), unsupported());
        let off = deepseek(DeepSeekConfig::new("fixture", "fixture"), Effort::Disabled).unwrap();
        assert_eq!(off.thinking, Some(ThinkingMode::Disabled));
        assert_eq!(off.reasoning_effort, None);
    }
}
