use adk_model::{
    anthropic::{AnthropicConfig, Effort as AnthropicEffort, ThinkingMode},
    gemini::{ThinkingConfig, ThinkingLevel},
    openai::OpenAIReasoningEffort,
};
use sailry_protocol::{Effort, ErrorCode, Fault};

fn unsupported() -> Fault {
    Fault::new(
        ErrorCode::InvalidRequest,
        "reasoning choice is unsupported by this API",
    )
}

pub(super) fn deepseek(
    config: adk_model::deepseek::DeepSeekConfig,
    effort: Effort,
) -> Result<adk_model::deepseek::DeepSeekConfig, Fault> {
    use adk_model::deepseek::{ReasoningEffort, ThinkingMode};
    Ok(match effort {
        Effort::Default => config,
        Effort::Disabled => config.with_thinking_mode(ThinkingMode::Disabled),
        Effort::Low | Effort::High | Effort::Max => config
            .with_thinking_mode(ThinkingMode::Enabled)
            .with_reasoning_effort(match effort {
                Effort::Low => ReasoningEffort::Low,
                Effort::High => ReasoningEffort::High,
                _ => ReasoningEffort::Max,
            }),
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
    fn deepseek_preserves_low_and_disabled() {
        use adk_model::deepseek::{DeepSeekConfig, ReasoningEffort, ThinkingMode};
        let low = deepseek(DeepSeekConfig::new("fixture", "fixture"), Effort::Low).unwrap();
        assert_eq!(low.reasoning_effort, Some(ReasoningEffort::Low));
        assert_eq!(low.thinking, Some(ThinkingMode::Enabled));
        let off = deepseek(DeepSeekConfig::new("fixture", "fixture"), Effort::Disabled).unwrap();
        assert_eq!(off.thinking, Some(ThinkingMode::Disabled));
        assert_eq!(off.reasoning_effort, None);
    }
}
