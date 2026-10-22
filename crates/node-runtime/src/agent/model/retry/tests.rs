use super::*;

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
    assert_eq!(
        super::super::error(&error)
            .message
            .matches("connection closed")
            .count(),
        1
    );
    assert!(matches!(
        classification::Failure::new(&error).class,
        classification::Class::Terminal(Stop::LocalFailure)
    ));
}
