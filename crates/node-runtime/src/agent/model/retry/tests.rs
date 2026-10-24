use super::*;
use sailry_protocol::RequestId;

#[test]
fn retains_only_last_cause_and_typed_stop() {
    let error = AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::Unavailable,
        "model.fixture.last",
        "last upstream error",
    )
    .with_upstream_status(503);
    let final_error = super::super::error(terminal(error, Stop::AttemptsExhausted));
    assert_eq!(
        final_error.message.matches("last upstream error").count(),
        1
    );
    assert!(matches!(
        final_error.details,
        Some(sailry_protocol::FaultDetails::ModelRecovery {
            reason: Stop::AttemptsExhausted
        })
    ));
    let uncertain = AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::Unavailable,
        "model.fixture.uncertain",
        "uncertain",
    )
    .with_source(Fault::new(ErrorCode::OutcomeUnknown, "uncertain result"));
    assert_eq!(
        super::super::error(terminal(uncertain, Stop::LocalFailure)).code,
        ErrorCode::OutcomeUnknown
    );
}

#[test]
fn preserves_local_fault_details() {
    let mut fault = Fault::new(ErrorCode::RevisionConflict, "plugin value changed");
    fault.details = Some(sailry_protocol::FaultDetails::PluginValueConflict);
    let error = AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::InvalidInput,
        "model.plugin.translation",
        "translation failed",
    )
    .with_source(fault.clone());
    assert_eq!(
        super::super::error(terminal(error, Stop::LocalFailure)),
        fault
    );
}

#[test]
fn preserves_completed_provider_tool_boundary() {
    let mut error = AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::Unavailable,
        "model.fixture.disconnected",
        "connection closed",
    );
    error
        .details
        .metadata
        .insert("provider_tool_completed".into(), true.into());
    let error = interrupted(error);
    assert_eq!(super::super::error(&error).code, ErrorCode::OutcomeUnknown);
    assert_eq!(super::super::error(&error).message, "connection closed");
    assert!(matches!(
        classification::Failure::new(&error).class,
        classification::Class::Terminal(Stop::LocalFailure)
    ));
}

#[test]
fn retains_recovery_context_and_safe_cause() {
    use crate::logging::{Area, Entry, Phase, Span};
    let mut base = Entry::new(Area::Model, Phase::Begin);
    base.context.call = Some(RequestId::new());
    let mut error = AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::Unavailable,
        "model.fixture.failed",
        "PRIVATE_PROVIDER_MESSAGE",
    )
    .with_upstream_status(503);
    error
        .details
        .metadata
        .insert("transport".into(), "websocket".into());
    let notice = Retry {
        attempt: 4,
        fallback: true,
        limit: Some(4),
        delay_ms: Some(250),
        transport: Some(Transport::WebSocket),
        status: Some(503),
        code: Some("PRIVATE_PROVIDER_CODE".into()),
        error: Some("PRIVATE_PROVIDER_MESSAGE".into()),
    };
    let entry = diagnostic(&base, &error, &notice);
    assert!(matches!(entry.phase, Phase::Retrying));
    assert_eq!(entry.context.call, base.context.call);
    assert_eq!(entry.wait_ms, Some(250));
    assert_eq!(entry.transport, Some("websocket"));
    assert_eq!(entry.reason, Some("backoff"));
    assert_eq!(entry.recovery_decision, Some("fallback"));
    assert_eq!(entry.fallback_reason, Some("attempts_exhausted"));
    assert_eq!(entry.cause_code, Some("model.fixture.failed"));
    assert_eq!(entry.cause_category, Some("unavailable"));
    assert_eq!(entry.upstream_status, Some(503));
    let mut final_call = Span::new(base.clone());
    final_call.entry.recovery_decision = Some(decision(Stop::AttemptsExhausted));
    classify(&mut final_call.entry, &error);
    super::super::diagnostics::finish(
        &mut final_call,
        &terminal(error, Stop::AttemptsExhausted),
        false,
    );
    assert!(matches!(final_call.entry.phase, Phase::Failed));
    assert_eq!(final_call.entry.context.call, base.context.call);
    assert_eq!(
        final_call.entry.recovery_decision,
        Some("attempts_exhausted")
    );
    assert_eq!(final_call.entry.cause_code, entry.cause_code);
    let mut success = Span::new(base);
    success.entry.recovery_decision = Some("succeeded");
    success.finish(Phase::Completed, None);
    assert!(matches!(success.entry.phase, Phase::Completed));
    assert_eq!(success.entry.context.call, entry.context.call);
}
