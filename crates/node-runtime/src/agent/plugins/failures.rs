//! Preserve the host fault when ADK constructs a failed tool's function response.
use sailry_protocol::{ErrorCode, Fault, FaultDetails, notification::DiagnosticReason};
use serde_json::{Value, json};

pub(in crate::agent) fn bind() -> adk_core::OnToolErrorCallback {
    Box::new(|context, _, _, _| {
        Box::pin(async move {
            Ok(context
                .tool_outcome()
                .and_then(|outcome| outcome.error)
                .and_then(|error| project(&error)))
        })
    })
}

fn project(error: &adk_core::AdkError) -> Option<Value> {
    let fault = std::error::Error::source(error)?.downcast_ref::<Fault>()?;
    let Some(FaultDetails::SystemDiagnostic(_)) = &fault.details else {
        return None;
    };
    let public = public(fault.clone());
    Some(json!({"isError":true,"error":public}))
}

pub(super) fn presentation(error: &adk_core::AdkError) -> Value {
    project(error)
        .map(|result| result["error"].clone())
        .unwrap_or_else(|| json!(Fault::invalid("plugin callback failed")))
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
        fault.details = Some(FaultDetails::SystemDiagnostic(report.clone()));
        let error = adk_core::AdkError::tool(fault.message.clone()).with_source(fault.clone());
        let projected = project(&error).unwrap();
        assert_eq!(projected["isError"], true);
        let restored: Fault = serde_json::from_value(projected["error"].clone()).unwrap();
        assert_eq!(restored.code, ErrorCode::OutcomeUnknown);
        assert_eq!(restored.details, fault.details);
        assert!(!projected.to_string().contains("PRIVATE_SDK_ERROR"));
        assert!(restored.message.contains("do not repeat"));
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<Fault>(),
            Some(&fault)
        );
        assert!(project(&adk_core::AdkError::tool("unrelated")).is_none());
    }
}
