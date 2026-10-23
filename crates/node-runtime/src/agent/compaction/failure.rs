//! Persist bounded diagnostics outside model content through the canonical event service.
use super::*;
use sailry_protocol::conversation::compaction::{Failure, Reason, Source, Stage};

const CLASSIFICATION: &str = "sailry.compaction.classification";

pub(super) fn annotate(
    mut error: adk_core::AdkError,
    stage: Stage,
    reason: Reason,
) -> adk_core::AdkError {
    error
        .details
        .metadata
        .insert(CLASSIFICATION.into(), json!([stage, reason]));
    error
}

pub(super) fn classify(error: &adk_core::AdkError) -> (Stage, Reason) {
    error
        .details
        .metadata
        .get(CLASSIFICATION)
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or((Stage::Unknown, Reason::Unknown))
}

/// Keep the standard error fields when an adapter embeds a JSON response body.
/// Other response fields may contain request echoes and are not diagnostics.
pub(super) fn diagnostic(message: &str) -> String {
    let Some(mut start) = message.find('{') else {
        return message.to_owned();
    };
    if message[..start].trim_end().ends_with('[') {
        start = message[..start].rfind('[').unwrap();
    }
    let parsed = serde_json::from_str::<serde_json::Value>(&message[start..]).ok();
    let error = parsed.as_ref().and_then(|value| {
        value
            .get("error")
            .or_else(|| value.get(0).and_then(|item| item.get("error")))
            .or_else(|| value.get("message").is_some().then_some(value))
    });
    let fields: serde_json::Map<String, serde_json::Value> = ["message", "code", "type"]
        .into_iter()
        .filter_map(|key| {
            let value = error?.get(key)?;
            (value.is_string() || value.is_number()).then(|| (key.into(), value.clone()))
        })
        .collect();
    if fields.is_empty() {
        return format!("{}[upstream response body omitted]", &message[..start]);
    }
    format!("{}{}", &message[..start], serde_json::Value::Object(fields))
}

fn cause(error: &adk_core::AdkError) -> Option<String> {
    // Unknown failures may carry arbitrary storage or execution context. Only
    // request diagnostics and Sailry-owned validation messages are public.
    (classify(error).0 != Stage::Unknown).then(|| model::error(error).message)
}

pub(super) async fn record(
    service: &dyn SessionService,
    session: &str,
    turn: TurnId,
    mode: SummaryMode,
    error: &adk_core::AdkError,
    continued: bool,
) -> adk_core::Result<()> {
    let (stage, reason) = classify(error);
    let failure = Failure {
        source: match mode {
            SummaryMode::Automatic => Source::Automatic,
            SummaryMode::Manual => Source::Manual,
        },
        stage,
        reason,
        continued,
        cause: cause(error),
        status: error
            .details
            .upstream_status_code
            .filter(|status| (100..=599).contains(status)),
    };
    let mut event = Event::new(turn.to_string());
    event.author = "system".into();
    event.provider_metadata.insert(
        "sailry_compaction_failure".into(),
        serde_json::to_string(&failure)
            .map_err(|_| invalid("context failure could not be encoded"))?,
    );
    service.append_event(session, event).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_response_echoes() {
        let message = "Provider API failed: {\"error\":{\"message\":\"Service unavailable [redacted]\",\"code\":\"fixture\",\"type\":\"server_error\",\"prompt\":\"PRIVATE_PROMPT\"},\"response\":\"PRIVATE_BODY\"}";
        let cause = diagnostic(message);
        assert!(cause.contains("Service unavailable [redacted]"));
        assert!(cause.contains("fixture"));
        assert!(!cause.contains("PRIVATE"));
        assert_eq!(
            diagnostic("Bearer [redacted] failed"),
            "Bearer [redacted] failed"
        );
    }

    #[test]
    fn distinguishes_unknown_save_and_truncation() {
        assert_eq!(
            classify(&invalid("unrecognized internal failure")),
            (Stage::Unknown, Reason::Unknown)
        );
        assert_eq!(
            classify(&save_error(adk_core::AdkError::session(
                "private storage path"
            ))),
            (Stage::Persistence, Reason::PersistenceFailed)
        );
        assert_eq!(
            classify(&invalid_at(
                Stage::Summary,
                Reason::TruncatedSummary,
                "context summary was truncated"
            )),
            (Stage::Summary, Reason::TruncatedSummary)
        );
        assert!(
            !model::error(save_error(adk_core::AdkError::session(
                "private storage path"
            )))
            .message
            .contains("private")
        );
    }

    #[test]
    fn ignores_diagnostic_wording() {
        let mut error = invalid_at(
            Stage::Summary,
            Reason::TruncatedSummary,
            "context summary was truncated",
        );
        error.message = "Updated diagnostic wording".into();
        assert_eq!(classify(&error), (Stage::Summary, Reason::TruncatedSummary));
        assert_eq!(
            classify(&invalid("context summary was truncated")),
            (Stage::Unknown, Reason::Unknown)
        );
    }

    #[test]
    fn preserves_request_cause_without_metadata() {
        use adk_core::ErrorCategory;
        for (category, reason) in [
            (ErrorCategory::Timeout, Reason::RequestTimeout),
            (ErrorCategory::RateLimited, Reason::RateLimited),
            (ErrorCategory::Unauthorized, Reason::AuthorizationFailed),
            (ErrorCategory::Unavailable, Reason::ServiceRejected),
        ] {
            let mut error = adk_core::AdkError::new(
                adk_core::ErrorComponent::Model,
                category,
                "fixture.private",
                "The upstream service is temporarily unavailable",
            )
            .with_upstream_status(503);
            error
                .details
                .metadata
                .insert("body".into(), json!("private response"));
            let safe = request_error(error);
            assert_eq!(classify(&safe), (Stage::Request, reason));
            let failure = Failure {
                source: Source::Automatic,
                stage: Stage::Request,
                reason,
                continued: true,
                status: Some(503),
                cause: cause(&safe),
            };
            let value = failure.resource().to_string();
            assert!(!value.contains("private response"));
            assert!(value.contains("temporarily unavailable"));
            assert!(value.contains("fixture.private"));
            assert!(value.contains("HTTP 503"));
        }
    }
}
