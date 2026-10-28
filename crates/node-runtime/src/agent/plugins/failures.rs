//! Preserve public classification while keeping private plugin diagnostics out of history.
use sailry_protocol::{ErrorCode, Fault, FaultDetails, notification::DiagnosticReason};
use serde_json::{Value, json};

pub(super) fn presentation(error: &Fault) -> Value {
    if matches!(&error.details, Some(FaultDetails::SystemDiagnostic(_))) {
        json!(public(error.clone()))
    } else {
        json!(Fault::invalid("plugin callback failed"))
    }
}

pub(in crate::agent) fn public(mut fault: Fault) -> Fault {
    let Some(FaultDetails::SystemDiagnostic(diagnostic)) = &fault.details else {
        return fault;
    };
    // Preserve the original source internally; durable public history is metadata-only.
    fault.message = if fault.code == ErrorCode::OutcomeUnknown {
        "plugin SDK completion is unknown; do not repeat this operation"
    } else if diagnostic.reason == DiagnosticReason::Timeout {
        "plugin response timed out"
    } else if diagnostic.reason == DiagnosticReason::Cancelled {
        "plugin operation stopped"
    } else {
        "plugin callback failed"
    }
    .into();
    fault
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::{
        RequestId,
        notification::{SystemDiagnostic, SystemSource},
    };
    #[test]
    fn preserves_classification_without_private_errors() {
        let mut fault = Fault::new(ErrorCode::OutcomeUnknown, "PRIVATE_SDK_ERROR");
        let mut report =
            SystemDiagnostic::new(RequestId::new(), SystemSource::PluginCallback, fault.code);
        report.reason = DiagnosticReason::Timeout;
        fault.details = Some(FaultDetails::SystemDiagnostic(Box::new(report.clone())));
        let projected = presentation(&fault);
        let restored: Fault = serde_json::from_value(projected.clone()).unwrap();
        assert_eq!(restored.code, ErrorCode::OutcomeUnknown);
        assert_eq!(restored.details, fault.details);
        assert!(!projected.to_string().contains("PRIVATE_SDK_ERROR"));
        assert!(restored.message.contains("do not repeat"));
        assert_eq!(
            presentation(&Fault::invalid("unrelated")),
            json!(Fault::invalid("plugin callback failed"))
        );
    }
}
