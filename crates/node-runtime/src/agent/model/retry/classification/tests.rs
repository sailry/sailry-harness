use super::*;
use adk_core::{ErrorCategory, ErrorComponent};

fn failure(
    status: Option<u16>,
    event: Option<&str>,
    code: Option<&str>,
    kind: Option<&str>,
) -> AdkError {
    let mut error = AdkError::new(
        ErrorComponent::Model,
        ErrorCategory::Unavailable,
        "model.fixture.error",
        "original cause",
    );
    error.details.upstream_status_code = status;
    for (key, value) in [
        ("provider_event_type", event),
        ("provider_error_code", code),
        ("provider_error_type", kind),
    ] {
        if let Some(value) = value {
            error.details.metadata.insert(key.into(), value.into());
        }
    }
    error
}

#[test]
fn distinguishes_http_and_stream_mappings() {
    for (status, code, kind, expected) in [
        (401, None, None, Class::Backoff),
        (403, None, None, Class::Backoff),
        (400, None, None, Class::Terminal(Stop::InvalidRequest)),
        (429, None, None, Class::Advice(Stop::RateLimited)),
        (
            429,
            None,
            Some("insufficient_quota"),
            Class::Terminal(Stop::QuotaExceeded),
        ),
        (
            429,
            Some("organization_usage_limit_exceeded"),
            None,
            Class::Terminal(Stop::QuotaExceeded),
        ),
        (
            429,
            None,
            Some("usage_limit_reached"),
            Class::Terminal(Stop::UsageLimitReached),
        ),
        (
            503,
            Some("server_is_overloaded"),
            None,
            Class::Advice(Stop::ServerOverloaded),
        ),
        (503, Some("slow_down"), None, Class::Backoff),
        (599, Some("future_failure"), None, Class::Backoff),
    ] {
        assert_eq!(
            Failure::new(&failure(Some(status), None, code, kind)).class,
            expected
        );
    }
    for (code, expected) in [
        ("insufficient_quota", Class::Terminal(Stop::QuotaExceeded)),
        ("organization_usage_limit_exceeded", Class::Backoff),
        ("subscription_sharing_usage_limit_exceeded", Class::Backoff),
        ("unknown_backend_quota", Class::Backoff),
        (
            "context_length_exceeded",
            Class::Terminal(Stop::ContextWindowExceeded),
        ),
    ] {
        assert_eq!(
            Failure::new(&failure(None, Some("response.failed"), Some(code), None)).class,
            expected
        );
    }
}

#[test]
fn protects_completed_and_local_failures() {
    let mut error = failure(None, Some("response.completed"), None, None);
    error
        .details
        .metadata
        .insert("response_completed".into(), true.into());
    assert_eq!(
        Failure::new(&error).class,
        Class::Terminal(Stop::ResponseIncomplete)
    );
    error = AdkError::new(
        ErrorComponent::Model,
        ErrorCategory::InvalidInput,
        "model.fixture.configuration",
        "invalid local configuration",
    );
    assert_eq!(
        Failure::new(&error).class,
        Class::Terminal(Stop::LocalFailure)
    );
    error = failure(None, None, None, None)
        .with_source(Fault::new(ErrorCode::OutcomeUnknown, "unknown result"));
    assert_eq!(
        Failure::new(&error).class,
        Class::Terminal(Stop::LocalFailure)
    );
}

#[test]
fn distinguishes_connection_from_stream_errors() {
    for (kind, expected) in [
        ("connection", Class::Connection),
        ("network", Class::Backoff),
        ("timeout", Class::Backoff),
        ("build", Class::Backoff),
    ] {
        let mut error = failure(None, None, None, None);
        error
            .details
            .metadata
            .insert("transport_error_kind".into(), kind.into());
        assert_eq!(Failure::new(&error).class, expected);
    }
}

#[tokio::test(start_paused = true)]
async fn limits_text_advice_to_rate_codes() {
    let mut error = failure(None, Some("response.failed"), Some("slow_down"), None);
    error.message = "Please try again in 1.25 seconds".into();
    let now = Instant::now();
    assert_eq!(
        Failure::new(&error).retry_at.unwrap().duration_since(now),
        std::time::Duration::from_millis(1250)
    );
    error = error.with_retry_deadline((now + std::time::Duration::from_secs(8)).into_std());
    assert_eq!(
        Failure::new(&error).retry_at,
        Some(now + std::time::Duration::from_secs(8))
    );
    let mut unknown = failure(None, Some("response.failed"), Some("relay_quota"), None);
    unknown.message = "Please try again in 117 seconds".into();
    assert!(Failure::new(&unknown).retry_at.is_none());
}
