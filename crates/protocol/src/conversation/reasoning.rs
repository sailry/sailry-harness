//! Model-declared choices are shared by configuration, roles and turn admission.
use super::{Model, ModelApi};
use crate::{Effort, ErrorCode, Fault};
#[cfg(test)]
mod tests;

impl Effort {
    /// Resolve an explicit choice, or omit the parameter when no choices exist.
    pub fn initial(choices: &[Self], preferred: Self) -> Self {
        if preferred != Self::Default && choices.contains(&preferred) {
            preferred
        } else {
            choices
                .iter()
                .copied()
                .find(|value| *value != Self::Default)
                .unwrap_or(Self::Default)
        }
    }

    /// Check the API surface and output limit, without guessing model capabilities.
    pub fn validate(self, api: ModelApi, output: u32) -> Result<(), Fault> {
        let supported = match (api, self) {
            (ModelApi::AzureAi | ModelApi::Bedrock, Self::Default) => true,
            (ModelApi::AzureAi | ModelApi::Bedrock, _) => false,
            (ModelApi::AzureOpenAi, effort) => {
                return effort.validate(ModelApi::ChatCompletions, output);
            }
            (ModelApi::Vertex, effort) => return effort.validate(ModelApi::Gemini, output),
            (
                ModelApi::DeepSeek,
                Self::Default | Self::Disabled | Self::Low | Self::High | Self::Max,
            ) => true,
            (ModelApi::DeepSeek, _) => false,
            (ModelApi::OpenCodeGo, Self::Budget(tokens)) => {
                tokens >= 1024 && (tokens as u32) < output
            }
            (ModelApi::OpenCodeZen, Self::Budget(tokens)) => tokens == -1 || tokens > 0,
            (ModelApi::OpenCodeGo | ModelApi::OpenCodeZen, _) => true,
            (_, Self::Default | Self::Low | Self::Medium | Self::High) => true,
            (ModelApi::ChatCompletions | ModelApi::Responses, Self::Budget(_)) => false,
            (ModelApi::ChatCompletions | ModelApi::Responses, _) => true,
            (ModelApi::Anthropic, Self::XHigh | Self::Max) => true,
            (ModelApi::Anthropic, Self::Budget(tokens)) => {
                tokens >= 1024 && (tokens as u32) < output
            }
            (ModelApi::Gemini, Self::Disabled | Self::Minimal) => true,
            (ModelApi::Gemini, Self::Budget(tokens)) => tokens == -1 || tokens > 0,
            _ => false,
        };
        if supported {
            Ok(())
        } else {
            Err(Fault::new(
                ErrorCode::InvalidRequest,
                "reasoning choice is unsupported by the API or output limit",
            ))
        }
    }
}

impl Model {
    pub fn validate_reasoning(&self, api: ModelApi) -> Result<(), Fault> {
        if !self.reasoning {
            return Ok(());
        }
        if self.efforts.len() > 16
            || if self.efforts.is_empty() {
                self.default_effort != Effort::Default
            } else {
                !self.efforts.contains(&self.default_effort)
            }
            || self
                .efforts
                .iter()
                .enumerate()
                .any(|(index, effort)| self.efforts[..index].contains(effort))
        {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "reasoning choices must be unique, bounded and include the default",
            ));
        }
        for effort in &self.efforts {
            effort.validate(api, self.output)?;
        }
        Ok(())
    }

    pub fn validate_effort(&self, api: ModelApi, effort: Effort) -> Result<(), Fault> {
        if self.reasoning {
            if !(self.efforts.contains(&effort)
                || (self.efforts.is_empty() && effort == Effort::Default))
            {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "reasoning choice is not configured for this model",
                ));
            }
            effort.validate(api, self.output)?;
        }
        Ok(())
    }
}
