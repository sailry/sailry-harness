//! Request customization at the native SDK dispatch boundary.

use adk_anthropic::{Anthropic, MessageCreateParams};
use adk_core::{AdkError, ErrorCategory, ErrorComponent};
use http::{HeaderMap, HeaderValue, header};

/// Adapt serialized message parameters and HTTP headers before dispatch.
///
/// Changes must remain valid `MessageCreateParams` and preserve the requested
/// response mode. The adapter must not perform I/O. It runs for each HTTP attempt
/// under the model's configured retry policy.
pub type RequestAdapter = std::sync::Arc<
    dyn Fn(&mut serde_json::Value, &mut HeaderMap) -> Result<(), AdkError> + Send + Sync,
>;

pub(super) fn prepare(
    client: &Anthropic,
    params: MessageCreateParams,
    adapter: &RequestAdapter,
) -> Result<(MessageCreateParams, HeaderMap), AdkError> {
    let streaming = params.stream;
    let mut headers = client.default_headers_for_request();
    if params.stream {
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );
    }
    // The pinned SDK's message_headers is private. Match its normal preparation
    // before the adapter gets final control over replacement headers.
    for (required, beta) in [
        (
            params.requires_structured_outputs_beta(),
            "structured-outputs-2025-11-13",
        ),
        (
            params.context_management.is_some(),
            "context-management-2025-06-27",
        ),
        (params.speed.is_some(), "fast-mode-2026-02-01"),
    ] {
        if required {
            append_beta(&mut headers, beta)?;
        }
    }
    let mut body = serde_json::to_value(params)
        .map_err(|_| invalid_request("Could not serialize Anthropic message parameters"))?;
    adapter(&mut body, &mut headers)?;
    let params: MessageCreateParams = serde_json::from_value(body).map_err(|_| {
        invalid_request("Request adapter produced invalid Anthropic message parameters")
    })?;
    if params.stream != streaming {
        return Err(invalid_request(
            "Request adapter changed Anthropic response mode",
        ));
    }
    Ok((params, headers))
}

fn append_beta(headers: &mut HeaderMap, beta: &str) -> Result<(), AdkError> {
    let existing = headers
        .get("anthropic-beta")
        .map(|value| value.to_str())
        .transpose()
        .map_err(|_| invalid_request("Invalid Anthropic beta header"))?
        .unwrap_or_default();
    if existing.split(',').any(|value| value.trim() == beta) {
        return Ok(());
    }
    let combined = if existing.is_empty() {
        beta.to_owned()
    } else {
        format!("{existing},{beta}")
    };
    let value = HeaderValue::from_str(&combined)
        .map_err(|_| invalid_request("Invalid Anthropic beta header"))?;
    headers.insert("anthropic-beta", value);
    Ok(())
}

fn invalid_request(message: &str) -> AdkError {
    AdkError::new(
        ErrorComponent::Model,
        ErrorCategory::InvalidInput,
        "model.anthropic.request_adapter",
        message,
    )
    .with_provider("anthropic")
}

#[cfg(test)]
mod tests;
