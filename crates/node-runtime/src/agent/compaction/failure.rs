//! Persist bounded diagnostics outside model content through the canonical event service.
use super::*;
use sailry_protocol::conversation::compaction::{Failure, Reason, Source, Stage};

pub(super) fn classify(error: &adk_core::AdkError) -> (Stage, Reason) {
    if error.message == "context summary request failed" {
        use adk_core::ErrorCategory;
        return (
            Stage::Request,
            match error.category {
                ErrorCategory::Timeout => Reason::RequestTimeout,
                ErrorCategory::RateLimited => Reason::RateLimited,
                ErrorCategory::Unauthorized | ErrorCategory::Forbidden => {
                    Reason::AuthorizationFailed
                }
                _ if error.details.upstream_status_code.is_some() => Reason::ServiceRejected,
                _ => Reason::RequestFailed,
            },
        );
    }
    match error.message.as_str() {
        "context summary was truncated" => (Stage::Summary, Reason::TruncatedSummary),
        "context summary could not be saved" => (Stage::Persistence, Reason::PersistenceFailed),
        "context summary is empty" => (Stage::Summary, Reason::EmptySummary),
        "model context does not match the persisted summary"
        | "persisted context summary is missing" => (Stage::History, Reason::HistoryMismatch),
        "context summary request failed" => (Stage::Request, Reason::RequestFailed),
        "context summary was not completed" | "context summary stream ended before completion" => {
            (Stage::Summary, Reason::IncompleteResponse)
        }
        "context summary returned non-text output" => (Stage::Summary, Reason::UnexpectedOutput),
        "context summary exceeds the size limit" => (Stage::Summary, Reason::SummaryTooLarge),
        "context summary did not reduce the conversation" => (Stage::Summary, Reason::NoReduction),
        "context exceeds the model limit and no summary was applied" => {
            (Stage::Budget, Reason::NoFittingPrefix)
        }
        message if message.contains("budget") || message.contains("no complete prefix") => {
            (Stage::Budget, Reason::NoFittingPrefix)
        }
        _ => (Stage::Unknown, Reason::Unknown),
    }
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
            classify(&invalid("context summary was truncated")),
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
    fn limits_request_diagnostics_to_safe_classifications() {
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
                "private token and conversation",
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
            };
            assert!(!failure.resource().to_string().contains("private"));
        }
    }
}
