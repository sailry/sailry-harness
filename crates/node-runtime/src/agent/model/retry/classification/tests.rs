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
        ("rate_limit_exceeded", Class::Advice(Stop::RateLimited)),
        ("slow_down", Class::Advice(Stop::RateLimited)),
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

#[tokio::test(start_paused = true)]
async fn separates_rate_limits_from_socket_backoff() {
    for code in ["rate_limit_exceeded", "slow_down"] {
        let mut error = failure(None, Some("response.failed"), Some(code), None);
        let classified = Failure::new(&error);
        assert!(matches!(
            super::super::budget::Budget::default().next(&classified, 5, false),
            super::super::budget::Decision::Stop(Stop::RateLimited)
        ));
        error.message = "Please try again in 60 seconds".into();
        let classified = Failure::new(&error);
        let now = Instant::now();
        let super::super::budget::Decision::Retry { at, delay, .. } =
            super::super::budget::Budget::default().next(&classified, 5, false)
        else {
            panic!("rate advice expected")
        };
        assert_eq!(at, now + std::time::Duration::from_secs(60));
        assert!(delay.is_none());
    }
    let error = failure(None, None, None, None);
    let classified = Failure::new(&error);
    let now = Instant::now();
    let super::super::budget::Decision::Retry { at, .. } =
        super::super::budget::Budget::default().next(&classified, 5, false)
    else {
        panic!("socket backoff expected")
    };
    assert!(at.duration_since(now) >= std::time::Duration::from_millis(180));
    assert!(at.duration_since(now) < std::time::Duration::from_millis(220));
}

#[test]
fn falls_back_only_before_definitive_refusal() {
    for (phase, refusal, expected) in [
        ("request", "definitive", true),
        ("request", "transient", false),
        ("stream", "definitive", false),
    ] {
        let mut error = AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Unavailable,
            "model.openai_responses.unfinished_stream",
            "socket closed",
        );
        for (key, value) in [
            ("transport", "websocket"),
            ("transport_phase", phase),
            ("websocket_refusal", refusal),
        ] {
            error.details.metadata.insert(key.into(), value.into());
        }
        assert_eq!(Failure::new(&error).refused, expected);
        if expected {
            let captured =
                error.with_source(Fault::new(ErrorCode::OutcomeUnknown, "captured effect"));
            assert!(!Failure::new(&captured).refused);
            continue;
        }
        error.code = "model.openai_responses.ws_serialize";
        assert!(!Failure::new(&error).refused);
    }
}

#[test]
fn preserves_accepted_socket_delivery_boundary() {
    let mut error = AdkError::new(
        ErrorComponent::Model,
        ErrorCategory::Unavailable,
        "model.openai_responses.unfinished_stream",
        "socket closed",
    );
    error
        .details
        .metadata
        .insert("transport".into(), "websocket".into());
    error
        .details
        .metadata
        .insert("transport_phase".into(), "stream".into());
    error.retry.should_retry = false;
    let classified = Failure::new(&error);
    assert_eq!(classified.class, Class::Terminal(Stop::ResponseIncomplete));
    assert!(!classified.refused);
    error
        .details
        .metadata
        .insert("transport_phase".into(), "request".into());
    error.retry.should_retry = true;
    assert_eq!(Failure::new(&error).class, Class::Backoff);
}

#[tokio::test(start_paused = true)]
async fn switches_only_safe_pre_event_failures() {
    for case in [
        "eligible",
        "advice",
        "rate",
        "authentication",
        "accepted",
        "build",
        "completed",
        "tool",
    ] {
        let mut error = AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Unavailable,
            "model.openai_responses.unfinished_stream",
            "socket closed",
        );
        error
            .details
            .metadata
            .insert("transport".into(), "websocket".into());
        error
            .details
            .metadata
            .insert("transport_phase".into(), "request".into());
        match case {
            "advice" => {
                error = error.with_retry_deadline(
                    (Instant::now() + std::time::Duration::from_secs(60)).into_std(),
                )
            }
            "rate" => error.details.upstream_status_code = Some(429),
            "authentication" => error.details.upstream_status_code = Some(401),
            "accepted" => {
                error
                    .details
                    .metadata
                    .insert("transport_phase".into(), "stream".into());
                error.retry.should_retry = false;
            }
            "build" => {
                error
                    .details
                    .metadata
                    .insert("transport_error_kind".into(), "build".into());
            }
            "completed" => {
                error
                    .details
                    .metadata
                    .insert("response_completed".into(), true.into());
            }
            "tool" => {
                error = error.with_source(Fault::new(ErrorCode::OutcomeUnknown, "captured effect"))
            }
            _ => {}
        }
        assert_eq!(Failure::new(&error).switch, case == "eligible", "{case}");
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
