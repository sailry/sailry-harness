//! Package deltas become the same messages and usage returned by native GDK providers.
use goose_provider_types::{
    conversation::{
        message::{Message, MessageContentBlock},
        token_usage::{ProviderUsage, Usage},
    },
    errors::ProviderError,
};
use rmcp::model::{CallToolRequestParams, ErrorData};
use sailry_protocol::{
    Fault, Secret,
    plugin::model_adapter::wire::{Class, Delta, Finish},
};
use std::collections::BTreeMap;

pub(super) type Chunk = (Option<Message>, Option<ProviderUsage>);
#[derive(Default)]
struct Call {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub(super) struct Accumulator {
    thinking: String,
    signature: String,
    calls: BTreeMap<u64, Call>,
    usage: Usage,
    reasoning: Option<u64>,
    finish: Option<Finish>,
}

impl Accumulator {
    pub(super) fn finished(&self) -> bool {
        self.finish.is_some()
    }

    pub(super) fn push(
        &mut self,
        deltas: Vec<Delta>,
        secrets: &[Secret],
    ) -> Result<Vec<Chunk>, ProviderError> {
        let mut output = Vec::new();
        for delta in deltas {
            match delta {
                Delta::Reasoning { text, replay } => {
                    if let Some(text) = text {
                        self.thinking.push_str(&text);
                    }
                    if let Some(signature) = replay {
                        self.signature.push_str(&signature);
                    }
                }
                Delta::Text { text } => {
                    self.flush_thinking(&mut output);
                    if !text.is_empty() {
                        output.push((Some(Message::assistant().with_text(text)), None));
                    }
                }
                Delta::ToolCall {
                    index,
                    id,
                    name,
                    arguments_delta,
                } => {
                    self.flush_thinking(&mut output);
                    let call = self.calls.entry(index).or_default();
                    if let Some(id) = id {
                        call.id = id;
                    }
                    if let Some(name) = name {
                        call.name.push_str(&name);
                    }
                    if let Some(arguments) = arguments_delta {
                        call.arguments.push_str(&arguments);
                    }
                }
                Delta::Usage {
                    input_tokens,
                    output_tokens,
                    cached_input_tokens,
                    reasoning_tokens,
                } => {
                    if let Some(value) = input_tokens {
                        self.usage.input_tokens = Some(count(value)?);
                    }
                    if let Some(value) = output_tokens {
                        self.usage.output_tokens = Some(count(value)?);
                    }
                    if let Some(value) = cached_input_tokens {
                        self.usage.cache_read_input_tokens = Some(count(value)?);
                    }
                    if let Some(value) = reasoning_tokens {
                        self.reasoning = Some(value);
                    }
                }
                Delta::Finish { reason } => self.finish = Some(reason),
                Delta::Error {
                    class,
                    message,
                    retry_after_ms,
                    ..
                } => return Err(classified(class, &message, retry_after_ms, None, secrets)),
            }
        }
        Ok(output)
    }

    fn flush_thinking(&mut self, output: &mut Vec<Chunk>) {
        if self.thinking.is_empty() && self.signature.is_empty() {
            return;
        }
        output.push((
            Some(
                Message::assistant().with_content(MessageContentBlock::thinking(
                    std::mem::take(&mut self.thinking),
                    std::mem::take(&mut self.signature),
                )),
            ),
            None,
        ));
    }

    pub(super) fn finish(mut self, model: &str) -> Result<Vec<Chunk>, ProviderError> {
        let reason = self.finish.ok_or_else(|| {
            ProviderError::NetworkError("model stream ended before completion".into())
        })?;
        let mut output = Vec::new();
        self.flush_thinking(&mut output);
        let mut message = Message::assistant();
        for (_, call) in self.calls {
            let arguments =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&call.arguments);
            let result = arguments
                .map(|arguments| CallToolRequestParams::new(call.name).with_arguments(arguments))
                .map_err(|_| {
                    ErrorData::invalid_params("tool arguments must be a JSON object", None)
                });
            message
                .content
                .push(MessageContentBlock::tool_request(call.id, result));
        }
        self.usage.total_tokens = self
            .usage
            .input_tokens
            .zip(self.usage.output_tokens)
            .map(|(input, output)| input.saturating_add(output));
        let mut usage = ProviderUsage::new(model.into(), self.usage).with_finish_reasons(vec![
            match reason {
                Finish::Stop => "stop",
                Finish::ToolCalls => "tool_calls",
                Finish::Length => "length",
                Finish::ContentFilter => "content_filter",
            }
            .into(),
        ]);
        usage.additional_data = self
            .reasoning
            .map(|tokens| [("reasoning_tokens".into(), tokens.into())].into());
        output.push((Some(message), Some(usage)));
        Ok(output)
    }
}

pub(super) fn http_class(status: u16) -> Class {
    match status {
        401 => Class::Authentication,
        403 => Class::Permission,
        429 => Class::RateLimited,
        408 | 500..=599 => Class::Unavailable,
        400..=499 => Class::InvalidRequest,
        _ => Class::Unknown,
    }
}

pub(super) fn classified(
    class: Class,
    message: &str,
    retry_after_ms: Option<u64>,
    status: Option<u16>,
    secrets: &[Secret],
) -> ProviderError {
    let message = super::super::errors::safe(message, secrets);
    match class {
        Class::Authentication | Class::Permission => ProviderError::Authentication(message),
        Class::RateLimited => ProviderError::RateLimitExceeded {
            details: message,
            retry_delay: retry_after_ms.map(std::time::Duration::from_millis),
        },
        Class::Quota => ProviderError::CreditsExhausted {
            details: message,
            top_up_url: None,
        },
        Class::ContextOverflow => ProviderError::ContextLengthExceeded(message),
        Class::InvalidRequest => ProviderError::InvalidValue(message),
        Class::Unsupported => ProviderError::NotImplemented(message),
        _ if status.is_some_and(|status| status >= 500) => ProviderError::ServerError(message),
        _ => ProviderError::NetworkError(message),
    }
}

pub(super) fn fault(error: Fault) -> ProviderError {
    super::super::errors::provider(error)
}
pub(super) fn resource() -> ProviderError {
    ProviderError::ExecutionError("model adapter resource budget exceeded".into())
}
pub(super) fn translation(error: Fault) -> ProviderError {
    fault(error)
}
pub(super) fn invalid(message: &'static str) -> ProviderError {
    ProviderError::InvalidValue(message.into())
}
fn count(value: u64) -> Result<i32, ProviderError> {
    i32::try_from(value).map_err(|_| invalid("model usage count is not representable"))
}
