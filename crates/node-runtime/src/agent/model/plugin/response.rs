//! Only complete, validated calls reach ADK's existing tool executor.
use adk_core::{
    AdkError, Content, ErrorCategory, ErrorComponent, FinishReason, LlmResponse, Part,
    UsageMetadata,
};
use sailry_protocol::{
    ErrorCode, Fault, Secret,
    plugin::model_adapter::wire::{Class, Delta, Finish},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
struct Call {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub(super) struct Accumulator {
    text: String,
    reasoning: String,
    replay: String,
    calls: BTreeMap<u64, Call>,
    usage: Option<UsageMetadata>,
    finish: Option<Finish>,
}

impl Accumulator {
    pub(super) fn finished(&self) -> bool {
        self.finish.is_some()
    }

    pub(super) fn push(
        &mut self,
        deltas: Vec<Delta>,
        streaming: bool,
        secrets: &[Secret],
    ) -> Result<Vec<LlmResponse>, AdkError> {
        let mut output = Vec::new();
        for delta in deltas {
            let mut partial = None;
            match delta {
                Delta::Text { text } => {
                    self.open()?;
                    self.text.push_str(&text);
                    if !text.is_empty() {
                        partial = Some(Part::Text { text });
                    }
                }
                Delta::Reasoning { text, replay } => {
                    self.open()?;
                    if let Some(text) = &text {
                        self.reasoning.push_str(text);
                    }
                    if let Some(replay) = &replay {
                        self.replay.push_str(replay);
                    }
                    if text.is_some() || replay.is_some() {
                        partial = Some(Part::Thinking {
                            thinking: text.unwrap_or_default(),
                            signature: replay,
                        });
                    }
                }
                Delta::ToolCall {
                    index,
                    id,
                    name,
                    arguments_delta,
                } => {
                    self.open()?;
                    let call = self.calls.entry(index).or_default();
                    if let Some(id) = id {
                        if !call.id.is_empty() && call.id != id {
                            return Err(invalid("model tool-call identity changed"));
                        }
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
                    let usage = self.usage.get_or_insert_with(UsageMetadata::default);
                    if let Some(value) = input_tokens {
                        usage.prompt_token_count = count(value)?;
                    }
                    if let Some(value) = output_tokens {
                        usage.candidates_token_count = count(value)?;
                    }
                    if let Some(value) = cached_input_tokens {
                        usage.cache_read_input_token_count = Some(count(value)?);
                    }
                    if let Some(value) = reasoning_tokens {
                        usage.thinking_token_count = Some(count(value)?);
                    }
                    usage.total_token_count = usage
                        .prompt_token_count
                        .checked_add(usage.candidates_token_count)
                        .ok_or_else(|| invalid("model usage count is not representable"))?;
                }
                Delta::Finish { reason } => {
                    if self.finish.is_some_and(|previous| previous != reason) {
                        return Err(invalid("model returned conflicting finish reasons"));
                    }
                    self.finish = Some(reason);
                }
                Delta::Error {
                    class,
                    message,
                    retry_after_ms,
                } => {
                    return Err(classified(class, &message, retry_after_ms, None, secrets));
                }
            }
            if streaming {
                output.push(LlmResponse {
                    content: partial.map(|part| Content {
                        role: "model".into(),
                        parts: vec![part],
                    }),
                    partial: true,
                    ..Default::default()
                });
            }
        }
        Ok(output)
    }

    fn open(&self) -> Result<(), AdkError> {
        if self.finish.is_some() {
            Err(invalid("model returned content after completion"))
        } else {
            Ok(())
        }
    }

    pub(super) fn finish(
        self,
        tools: &std::collections::BTreeSet<String>,
    ) -> Result<LlmResponse, AdkError> {
        let reason = self.finish.ok_or_else(|| {
            failure(
                ErrorCategory::Unavailable,
                "model.plugin.incomplete",
                "model stream ended before completion",
            )
        })?;
        if (!self.calls.is_empty() && matches!(reason, Finish::Length | Finish::ContentFilter))
            || (self.calls.is_empty() && reason == Finish::ToolCalls)
        {
            return Err(invalid("model did not complete its tool calls"));
        }
        let mut parts = Vec::new();
        if !self.reasoning.is_empty() || !self.replay.is_empty() {
            parts.push(Part::Thinking {
                thinking: self.reasoning,
                signature: (!self.replay.is_empty()).then_some(self.replay),
            });
        }
        if !self.text.is_empty() {
            parts.push(Part::Text { text: self.text });
        }
        let mut identities = std::collections::BTreeSet::new();
        for call in self.calls.into_values() {
            if call.id.is_empty()
                || call.name.is_empty()
                || !identities.insert(call.id.clone())
                || !tools.contains(&call.name)
            {
                return Err(invalid("model returned an invalid tool-call identity"));
            }
            let args: Value = serde_json::from_str(&call.arguments)
                .map_err(|_| invalid("model returned incomplete tool-call arguments"))?;
            if !args.is_object() {
                return Err(invalid("model tool-call arguments must be an object"));
            }
            parts.push(Part::FunctionCall {
                name: call.name,
                args,
                id: Some(call.id),
                thought_signature: None,
            });
        }
        Ok(LlmResponse {
            content: Some(Content {
                role: "model".into(),
                parts,
            }),
            usage_metadata: self.usage,
            finish_reason: Some(match reason {
                Finish::Stop | Finish::ToolCalls => FinishReason::Stop,
                Finish::Length => FinishReason::MaxTokens,
                Finish::ContentFilter => FinishReason::Safety,
                Finish::Other => FinishReason::Other,
            }),
            turn_complete: true,
            provider_metadata: Some(json!({"content_complete":true})),
            ..Default::default()
        })
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
) -> AdkError {
    let (category, code) = match class {
        Class::Authentication => (ErrorCategory::Unauthorized, "model.plugin.authentication"),
        Class::Permission => (ErrorCategory::Forbidden, "model.plugin.permission"),
        Class::RateLimited => (ErrorCategory::RateLimited, "model.plugin.rate_limited"),
        Class::Quota => (ErrorCategory::InvalidInput, "model.plugin.quota"),
        Class::ContextOverflow => (ErrorCategory::InvalidInput, "model.plugin.context_overflow"),
        Class::InvalidRequest => (ErrorCategory::InvalidInput, "model.plugin.invalid_request"),
        Class::Unavailable => (
            status
                .map(adk_model::retry::category_for_status_code)
                .filter(|category| {
                    matches!(
                        category,
                        ErrorCategory::Timeout | ErrorCategory::Unavailable
                    )
                })
                .unwrap_or(ErrorCategory::Unavailable),
            "model.plugin.unavailable",
        ),
        Class::Unsupported => (ErrorCategory::Unsupported, "model.plugin.unsupported"),
        Class::Unknown => (ErrorCategory::Unavailable, "model.plugin.unknown"),
    };
    let safe = redact(message, secrets);
    let mut error = failure(category, code, &safe);
    error.retry.retry_after_ms = retry_after_ms;
    if let Some(status) = status {
        error = error.with_upstream_status(status);
    }
    error
}

pub(super) fn fault(error: Fault) -> AdkError {
    let category = match error.code {
        ErrorCode::Cancelled => ErrorCategory::Cancelled,
        ErrorCode::PermissionDenied => ErrorCategory::Forbidden,
        ErrorCode::InvalidRequest => ErrorCategory::InvalidInput,
        ErrorCode::NotFound => ErrorCategory::NotFound,
        ErrorCode::Unavailable | ErrorCode::Busy => ErrorCategory::Unavailable,
        _ => ErrorCategory::Internal,
    };
    AdkError::new(
        ErrorComponent::Model,
        category,
        "model.plugin.runtime",
        error.message.clone(),
    )
    .with_source(error)
}

pub(super) fn resource() -> AdkError {
    let message = "model adapter resource budget exceeded";
    AdkError::new(
        ErrorComponent::Model,
        ErrorCategory::Internal,
        "model.plugin.resource_budget",
        message,
    )
    .with_source(Fault::new(ErrorCode::Internal, message))
}

pub(super) fn translation(error: Fault) -> AdkError {
    if error.code == ErrorCode::Cancelled {
        return fault(error);
    }
    let message = "model adapter execution failed or exhausted its resources";
    AdkError::new(
        ErrorComponent::Model,
        ErrorCategory::Internal,
        "model.plugin.translation",
        message,
    )
    .with_source(Fault::new(ErrorCode::Internal, message))
}

pub(super) fn invalid(message: &'static str) -> AdkError {
    failure(
        ErrorCategory::InvalidInput,
        "model.plugin.invalid_translation",
        message,
    )
}

fn failure(category: ErrorCategory, code: &'static str, message: &str) -> AdkError {
    let source = match category {
        ErrorCategory::Unauthorized | ErrorCategory::Forbidden => ErrorCode::PermissionDenied,
        ErrorCategory::InvalidInput => ErrorCode::InvalidRequest,
        ErrorCategory::Unsupported => ErrorCode::NotConfigured,
        ErrorCategory::Cancelled => ErrorCode::Cancelled,
        ErrorCategory::RateLimited | ErrorCategory::Unavailable => ErrorCode::Unavailable,
        _ => ErrorCode::Internal,
    };
    AdkError::new(ErrorComponent::Model, category, code, message)
        .with_source(Fault::new(source, message))
}

fn count(value: u64) -> Result<i32, AdkError> {
    i32::try_from(value).map_err(|_| invalid("model usage count is not representable"))
}

fn redact(value: &str, secrets: &[Secret]) -> String {
    let mut value = value.to_owned();
    let mut secrets: Vec<_> = secrets
        .iter()
        .map(Secret::expose)
        .filter(|secret| !secret.is_empty())
        .collect();
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    for secret in secrets {
        value = value.replace(secret, "[redacted]");
    }
    // Redaction precedes diagnostic display limits, including UTF-8 boundaries.
    if value.len() > 8192 {
        let mut end = 8192;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
    value
}
