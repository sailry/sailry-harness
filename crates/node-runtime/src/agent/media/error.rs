//! Only a definite pre-execution capability rejection permits the captured fallback.
use super::*;

#[derive(Debug)]
pub(super) enum Failure {
    Unsupported(Fault),
    Failed(Fault),
}

impl Failure {
    pub(super) fn unsupported() -> Self {
        Self::Unsupported(Fault::new(
            ErrorCode::NotConfigured,
            "model does not support this media operation",
        ))
    }

    pub(super) fn http(status: u16, body: &[u8], initial: bool) -> Self {
        let code = serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|value| {
                value
                    .pointer("/error/code")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        if initial && rejects_capability(status, code.as_deref()) {
            return Self::unsupported();
        }
        Fault::new(
            ErrorCode::Unavailable,
            format!("media endpoint returned HTTP {status}"),
        )
        .into()
    }

    pub(super) fn model(error: adk_core::AdkError, initial: bool) -> Self {
        let code = error
            .details
            .metadata
            .get("provider_error_code")
            .and_then(Value::as_str);
        if initial
            && error
                .details
                .upstream_status_code
                .is_some_and(|status| rejects_capability(status, code))
        {
            return Self::unsupported();
        }
        model::error(error).into()
    }

    pub(super) fn fault(self) -> Fault {
        match self {
            Self::Unsupported(fault) | Self::Failed(fault) => fault,
        }
    }
}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Self {
        Self::Failed(fault)
    }
}

fn rejects_capability(status: u16, code: Option<&str>) -> bool {
    match status {
        405 | 501 => true,
        400 | 422 => matches!(
            code,
            Some(
                "unsupported_model"
                    | "model_not_supported"
                    | "unsupported_tool"
                    | "tool_not_supported"
                    | "unsupported_capability"
                    | "unsupported_operation"
                    | "subscription_sharing_unsupported_capability"
            )
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_core::{AdkError, ErrorCategory, ErrorComponent};

    #[test]
    fn requires_a_definite_initial_rejection() {
        for status in [400, 422, 405, 501] {
            let body = br#"{"error":{"code":"unsupported_capability","message":"private"}}"#;
            assert!(matches!(
                Failure::http(status, body, true),
                Failure::Unsupported(_)
            ));
            assert!(matches!(
                Failure::http(status, body, false),
                Failure::Failed(_)
            ));
        }
        for status in [400, 404, 401, 403, 408, 429, 500, 503, 504] {
            let body =
                br#"{"error":{"code":"invalid_request","message":"unsupported private input"}}"#;
            let failure = Failure::http(status, body, true);
            assert!(matches!(failure, Failure::Failed(_)));
            assert!(!failure.fault().message.contains("private"));
        }
        for status in [401, 403, 429, 500, 503] {
            assert!(matches!(
                Failure::http(status, br#"{"error":{"code":"unsupported_model"}}"#, true),
                Failure::Failed(_)
            ));
        }
    }

    #[test]
    fn does_not_guess_from_model_messages() {
        for initial in [true, false] {
            let failure = AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::InvalidInput,
                "model.gemini.bad_request",
                "unsupported_capability private credential",
            )
            .with_upstream_status(400);
            assert!(matches!(
                Failure::model(failure, initial),
                Failure::Failed(_)
            ));
        }
        let mut failure = AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::InvalidInput,
            "model.gemini.bad_request",
            "private credential",
        )
        .with_upstream_status(400);
        failure.details.metadata.insert(
            "provider_error_code".into(),
            json!("unsupported_capability"),
        );
        assert!(matches!(
            Failure::model(failure, true),
            Failure::Unsupported(_)
        ));
    }

    #[test]
    fn never_replays_accepted_model_responses() {
        for status in [400, 422, 405, 501, 401, 403, 408, 429, 500, 503, 504] {
            let mut error = AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::InvalidInput,
                "model.fixture.invalid_request",
                "private credential",
            )
            .with_upstream_status(status);
            error.details.metadata.insert(
                "provider_error_code".into(),
                json!("unsupported_capability"),
            );
            assert!(matches!(Failure::model(error, false), Failure::Failed(_)));
        }
    }
}
