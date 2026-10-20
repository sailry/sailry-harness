use super::*;
use adk_core::{ErrorCategory, ErrorComponent};
use serde_json::{Value, json};

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
    let details = ProviderErrorDetails {
        message: Some(format!("Unsupported input 中文; Bearer {KEY}")),
        code: Some(format!("invalid_{KEY}")),
        kind: Some(format!("type_{KEY}")),
        param: Some(format!("input_{KEY}")),
        request_id: Some(format!("request_{KEY}")),
        status: Some(400),
    };
    let mut error = upstream();
    error
        .details
        .metadata
        .insert("provider_error_code".into(), json!(KEY));
    let error = redact(error, &[Secret::new(KEY.into())], Some(&details));
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
    let detail: Value = serde_json::from_str(detail).unwrap();
    assert_eq!(
        detail["message"],
        "Unsupported input 中文; Bearer [redacted]"
    );
    assert_eq!(detail["code"], "invalid_[redacted]");
    assert_eq!(detail["type"], "type_[redacted]");
    assert_eq!(detail["param"], "input_[redacted]");
    assert_eq!(detail["request_id"], "request_[redacted]");
    assert_eq!(detail["status"], 400);
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
    let details = ProviderErrorDetails {
        message: Some(format!("Account fixture-account; Bearer {KEY}")),
        ..Default::default()
    };
    let error = redact(
        upstream(),
        &[
            Secret::new(KEY.into()),
            Secret::new("fixture-account".into()),
        ],
        Some(&details),
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
    let error = redact(error, &[Secret::new(KEY.into())], None);
    assert_eq!(error.message, "Transport rejected [redacted]");
    assert_eq!(
        error.details.request_id.as_deref(),
        Some("request_[redacted]")
    );
    assert!(error.details.metadata.is_empty());
    assert!(!format!("{error:?}").contains(KEY));
    assert!(!error.to_problem_json().to_string().contains(KEY));
}
