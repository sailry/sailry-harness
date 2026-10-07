//! Responses error normalization and safe, non-replaying stream failures.

use adk_core::{AdkError, ErrorCategory, ErrorComponent, RetryHint};
use async_openai::types::responses::ResponseErrorEvent;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct NestedError {
    sequence_number: u64,
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: Option<String>,
    message: String,
    param: Option<String>,
}

pub(super) fn normalize_event(value: &mut Value) -> bool {
    if value.get("type").and_then(Value::as_str) != Some("error") || value.get("error").is_none() {
        return false;
    }
    // SIWC may nest the error fields. async-openai 0.41.3 requires the flat
    // ResponseErrorEvent schema even when the public OpenAI endpoint is used.
    let Ok(nested) = serde_json::from_value::<NestedError>(value.clone()) else {
        return false;
    };
    let event = ResponseErrorEvent {
        sequence_number: nested.sequence_number,
        code: nested.error.code,
        message: nested.error.message,
        param: nested.error.param,
    };
    let Ok(mut flattened) = serde_json::to_value(event) else {
        return false;
    };
    flattened["type"] = "error".into();
    *value = flattened;
    true
}

pub(super) fn sharing_error(code: Option<&str>) -> Option<AdkError> {
    let (category, diagnostic, message) = match code? {
        "subscription_sharing_usage_limit_exceeded" => (
            ErrorCategory::RateLimited,
            "model.openai_responses.subscription_sharing_usage_limit_exceeded",
            "ChatGPT shared usage limit exceeded",
        ),
        "subscription_sharing_usage_unavailable" => (
            ErrorCategory::Unavailable,
            "model.openai_responses.subscription_sharing_usage_unavailable",
            "ChatGPT shared usage is unavailable",
        ),
        _ => return None,
    };
    Some(
        AdkError::new(ErrorComponent::Model, category, diagnostic, message)
            .with_provider("openai-responses")
            .with_retry(RetryHint::default()),
    )
}

pub(super) fn stream_error(code: Option<&str>) -> AdkError {
    // Provider messages and unknown codes can echo request credentials. Only
    // known static diagnostics leave the adapter; no failed stream is replayed.
    sharing_error(code).unwrap_or_else(|| {
        AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Internal,
            "model.openai_responses.response_failed",
            "OpenAI Responses generation failed",
        )
        .with_provider("openai-responses")
    })
}
