use super::*;
use adk_core::{ErrorCategory, ErrorComponent};
use serde_json::json;

const KEY: &str = "fixture-private-access-token";

fn upstream() -> AdkError {
    AdkError::new(
        ErrorComponent::Model,
        ErrorCategory::InvalidInput,
        "model.openai_responses.invalid_request",
        "OpenAI Responses API request failed",
    )
    .with_upstream_status(400)
}

#[test]
fn redacts_captured_values_in_all_fields() {
    let mut error = upstream();
    error.message = format!("Unsupported input 中文; Bearer {KEY}");
    error.details.request_id = Some(format!("request_{KEY}"));
    for field in ["provider_error_code", "type", "param"] {
        error.details.metadata.insert(field.into(), json!(KEY));
    }
    let error = redact(error, &[Secret::new(KEY.into())]);
    assert!(error.details.metadata.is_empty());
    assert_eq!(
        error.details.request_id.as_deref(),
        Some("request_[redacted]")
    );
    assert!(!format!("{error:?}").contains(KEY));
    assert!(!error.to_problem_json().to_string().contains(KEY));
    let mut entry =
        crate::logging::Entry::new(crate::logging::Area::Model, crate::logging::Phase::Failed);
    super::super::diagnostics::cause(&mut entry, &error);
    assert_eq!(
        entry.cause_code,
        Some("model.openai_responses.invalid_request")
    );
    assert_eq!(entry.cause_component, Some("model"));
    assert_eq!(entry.cause_category, Some("invalid_input"));
    assert_eq!(entry.upstream_status, Some(400));
    assert_eq!(entry.parse_layer, None);
    assert_eq!(entry.parse_category, None);
    assert_eq!(entry.parse_cause, None);
    assert_eq!(entry.parse_field, None);
    assert_eq!(entry.parse_event, None);
    assert_eq!(entry.parse_line, None);
    assert_eq!(entry.parse_column, None);
    let fault = super::super::error(error);
    let (summary, detail) = fault.message.split_once("\n\n").unwrap();
    assert_eq!(
        summary,
        "Agent execution failed (model.openai_responses.invalid_request); HTTP 400; invalid_input"
    );
    assert_eq!(detail, "Unsupported input 中文; Bearer [redacted]");
}

#[test]
fn bounds_unicode_after_secret_redaction() {
    let message = format!("{}{}", "🙂".repeat(2047), KEY);
    let value = safe(&message, &[Secret::new(KEY.into())], 8192);
    assert!(!value.contains(KEY));
    assert!(!value.contains("fixture-private"));
    assert!(value.ends_with("... [truncated]"));
    assert!(value.len() <= 8192 + "... [truncated]".len());
    assert_eq!(
        safe(&"x".repeat(65537), &[], 8192),
        "[upstream diagnostic exceeded the size limit]"
    );
}

#[test]
fn protects_authorization_values() {
    let mut error = upstream();
    error.message = format!("Account fixture-account; Bearer {KEY}");
    let error = redact(
        error,
        &[
            Secret::new(KEY.into()),
            Secret::new("fixture-account".into()),
        ],
    );
    let fault = super::super::error(error);
    assert!(!fault.message.contains(KEY));
    assert!(!fault.message.contains("fixture-account"));
    assert!(
        fault
            .message
            .contains("Account [redacted]; Bearer [redacted]")
    );
}

#[test]
fn protects_errors_without_provider_fields() {
    let mut error = upstream();
    error.message = format!("Transport rejected {KEY}");
    error.details.request_id = Some(format!("request_{KEY}"));
    error.details.metadata.insert("private".into(), json!(KEY));
    let error = redact(error, &[Secret::new(KEY.into())]);
    assert_eq!(error.message, "Transport rejected [redacted]");
    assert_eq!(
        error.details.request_id.as_deref(),
        Some("request_[redacted]")
    );
    assert!(error.details.metadata.is_empty());
    assert!(!format!("{error:?}").contains(KEY));
    assert!(!error.to_problem_json().to_string().contains(KEY));
}

#[test]
fn preserves_native_retry_guidance() {
    for category in [
        ErrorCategory::RateLimited,
        ErrorCategory::Unavailable,
        ErrorCategory::Timeout,
        ErrorCategory::Unauthorized,
        ErrorCategory::Forbidden,
    ] {
        let mut error = upstream();
        error.category = category;
        error.retry = adk_core::RetryHint {
            should_retry: matches!(
                category,
                ErrorCategory::RateLimited | ErrorCategory::Unavailable | ErrorCategory::Timeout
            ),
            retry_after_ms: Some(7000),
            max_attempts: Some(3),
        };
        error
            .details
            .metadata
            .insert("provider_error_code".into(), json!("future_failure"));
        let before = serde_json::to_value(&error.details).unwrap();
        let retry = serde_json::to_value(&error.retry).unwrap();
        let error = redact(error, &[]);
        assert_eq!(error.category, category);
        assert_eq!(error.code, "model.openai_responses.invalid_request");
        assert_eq!(serde_json::to_value(&error.details).unwrap(), before);
        assert_eq!(serde_json::to_value(&error.retry).unwrap(), retry);
    }
}

#[test]
fn preserves_provider_codes_without_business_policy() {
    for code in [
        "subscription_sharing_usage_limit_exceeded",
        "subscription_sharing_usage_unavailable",
    ] {
        let mut error = AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Unavailable,
            "model.openai_responses.response_failed",
            "Upstream generation failed",
        );
        error
            .details
            .metadata
            .insert("provider_error_code".into(), json!(code));
        let error = redact(error, &[]);
        assert_eq!(error.category, ErrorCategory::Unavailable);
        assert_eq!(error.code, "model.openai_responses.response_failed");
        assert_eq!(error.details.metadata["provider_error_code"], code);
        assert_eq!(error.message, "Upstream generation failed");
    }
}

#[test]
fn protects_persisted_compaction_cause() {
    use sailry_protocol::conversation::compaction::{Failure, Reason, Source, Stage};
    let mut error = upstream();
    error.message = format!("Summary rejected; Bearer {KEY}");
    error
        .details
        .metadata
        .insert("body".into(), json!("private response and prompt"));
    let error = redact(error, &[Secret::new(KEY.into())]);
    let failure = Failure {
        source: Source::Manual,
        stage: Stage::Request,
        reason: Reason::ServiceRejected,
        continued: false,
        status: Some(400),
        cause: Some(super::super::error(error).message),
    };
    let value = failure.resource().to_string();
    assert!(value.contains("Summary rejected; Bearer [redacted]"));
    assert!(!value.contains(KEY));
    assert!(!value.contains("private response and prompt"));
}

#[tokio::test]
async fn protects_response_error_fields() {
    let response = adk_core::LlmResponse {
        error_code: Some(format!("failure_{KEY}")),
        error_message: Some(format!("Summary rejected; Bearer {KEY}")),
        ..Default::default()
    };
    let stream = Box::pin(futures::stream::iter([Ok(response)]));
    let response = protect(stream, vec![Secret::new(KEY.into())])
        .next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.error_code.as_deref(), Some("failure_[redacted]"));
    assert_eq!(
        response.error_message.as_deref(),
        Some("Summary rejected; Bearer [redacted]")
    );
}
