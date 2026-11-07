mod history;
mod output;

use super::super::types;
use rig_core::{
    completion::{CompletionRequest, CompletionResponse, ToolDefinition},
    message::{AssistantContent, Origin, ToolName},
};
use sailry_protocol::conversation::ModelApi;
use serde_json::Value;
use std::sync::Arc;
use types::{Content, Error, ErrorKind, Finish, Request, Usage};

pub(super) fn request(source: Request, api: ModelApi) -> Result<CompletionRequest, Error> {
    let (wire, provider) = match api {
        ModelApi::Responses => ("openai.responses", "openai"),
        ModelApi::ChatCompletions => ("openai.chat", "openai"),
        ModelApi::DeepSeek => ("openai.chat", "deepseek"),
        ModelApi::Anthropic => ("anthropic.messages", "anthropic"),
        ModelApi::Gemini => ("gemini.generate_content", "gcp.gemini"),
        _ => return Err(invalid("model route was not resolved")),
    };
    let origin = Origin::new(wire, provider, source.model.clone());
    let mut request = CompletionRequest::from(history::messages(source.messages, &origin)?);
    if !source.system.is_empty() {
        request = request.preamble(source.system);
    }
    request.model = Some(source.model);
    request.temperature = source.temperature;
    request.max_tokens = source.max_tokens;
    request.tools = source
        .tools
        .into_iter()
        .map(|tool| {
            let name = ToolName::new(tool.name).map_err(invalid)?;
            Ok(Arc::new(ToolDefinition::new(
                name,
                tool.description.unwrap_or_default(),
                tool.parameters,
            )))
        })
        .collect::<Result<_, Error>>()?;
    if source.effort != sailry_protocol::Effort::Default {
        request.options.reasoning = Some(reasoning(source.effort)?);
    }
    request.additional_params = (!source.params.is_empty()).then_some(Value::Object(source.params));
    if api == ModelApi::Anthropic {
        request.cache_points = request
            .chat_history
            .iter()
            .filter(|message| {
                !matches!(message.as_ref(), rig_core::message::Message::System { .. })
            })
            .enumerate()
            .filter_map(|(index, message)| {
                matches!(message.as_ref(), rig_core::message::Message::User { .. })
                    .then_some(rig_core::completion::CachePoint::Messages(index + 1))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(2)
            .collect();
    }
    request.validate_message_content().map_err(invalid)?;
    Ok(request)
}

pub(super) fn content(source: AssistantContent) -> Result<Content, Error> {
    output::content(source)
}

pub(super) fn usage(source: &CompletionResponse) -> Usage {
    let usage = source.usage;
    Usage {
        model: source
            .model()
            .map(str::to_owned)
            .or_else(|| Some(source.origin.model.clone())),
        input: usage.input_tokens,
        output: usage.output_tokens,
        total: usage.total_tokens,
        cache_read: usage.cached_input_tokens,
        cache_write: usage.cache_creation_input_tokens,
        reasoning: usage.reasoning_tokens,
        cost: usage
            .cost
            .filter(|cost| cost.is_complete())
            .map(|cost| cost.total),
        provider: source
            .raw
            .get("usage")
            .or_else(|| source.raw.get("usageMetadata"))
            .cloned(),
    }
}

pub(super) fn finish(source: Option<rig_core::completion::FinishReason>) -> Finish {
    use rig_core::completion::FinishReason;
    match source {
        Some(FinishReason::Stop) => Finish::Stop,
        Some(FinishReason::ToolCalls) => Finish::ToolCalls,
        Some(FinishReason::Length) => Finish::Length,
        Some(FinishReason::ContentFilter) => Finish::ContentFilter,
        Some(FinishReason::Other(reason)) => Finish::Other(reason),
        _ => Finish::Other("unknown".into()),
    }
}

fn reasoning(effort: sailry_protocol::Effort) -> Result<rig_core::completion::Reasoning, Error> {
    use rig_core::completion::{Effort, Reasoning};
    Ok(match effort {
        sailry_protocol::Effort::Disabled => Reasoning::Off,
        sailry_protocol::Effort::Minimal => Reasoning::Effort(Effort::Minimal),
        sailry_protocol::Effort::Low => Reasoning::Effort(Effort::Low),
        sailry_protocol::Effort::Medium => Reasoning::Effort(Effort::Medium),
        sailry_protocol::Effort::High => Reasoning::Effort(Effort::High),
        sailry_protocol::Effort::XHigh => Reasoning::Effort(Effort::XHigh),
        sailry_protocol::Effort::Max => Reasoning::Effort(Effort::Max),
        sailry_protocol::Effort::Budget(tokens) => Reasoning::Budget {
            tokens: u32::try_from(tokens).map_err(invalid)?,
        },
        sailry_protocol::Effort::Default => {
            return Err(invalid("default reasoning must remain unspecified"));
        }
    })
}

fn invalid(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::InvalidRequest, error.to_string())
}
