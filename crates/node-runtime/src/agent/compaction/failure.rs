//! Persist bounded diagnostics outside model content through the canonical event service.
use super::*;
use sailry_protocol::conversation::compaction::{Failure, Reason, Source, Stage};

const CLASSIFICATION: &str = "sailry.compaction.classification";

pub(super) fn request_reason(error: &adk_core::AdkError) -> Option<Reason> {
    use adk_core::ErrorCategory;
    match error.details.upstream_status_code {
        Some(401 | 403) => return Some(Reason::AuthorizationFailed),
        Some(429) => return Some(Reason::RateLimited),
        _ => {}
    }
    match error.category {
        ErrorCategory::Unauthorized | ErrorCategory::Forbidden => {
            return Some(Reason::AuthorizationFailed);
        }
        ErrorCategory::RateLimited => return Some(Reason::RateLimited),
        ErrorCategory::Cancelled => return None,
        _ => {}
    }
    match error
        .details
        .metadata
        .get("incomplete_reason")
        .and_then(serde_json::Value::as_str)
    {
        Some("content_filter") => return Some(Reason::ContentFiltered),
        Some("max_output_tokens") => return Some(Reason::TruncatedSummary),
        _ => {}
    }
    if model::context_overflow(error) {
        return Some(Reason::ContextOverflow);
    }
    None
}

pub(super) fn response_reason(response: &adk_core::LlmResponse) -> Option<Reason> {
    let reason = response.provider_metadata.as_ref().and_then(|metadata| {
        metadata
            .get("incomplete_reason")
            .or_else(|| metadata.pointer("/incomplete_details/reason"))
            .or_else(|| metadata.pointer("/openai/incomplete_details/reason"))
            .and_then(serde_json::Value::as_str)
    });
    match (reason, response.finish_reason) {
        (Some("content_filter"), _)
        | (_, Some(adk_core::FinishReason::Safety | adk_core::FinishReason::Recitation)) => {
            Some(Reason::ContentFiltered)
        }
        (Some("max_output_tokens"), _) | (_, Some(adk_core::FinishReason::MaxTokens)) => {
            Some(Reason::TruncatedSummary)
        }
        _ => None,
    }
}

pub(super) fn recoverable(error: &adk_core::AdkError) -> bool {
    if error.category == adk_core::ErrorCategory::Cancelled {
        return false;
    }
    let reason = request_reason(error).unwrap_or_else(|| classify(error).1);
    // More output budget cannot come from a smaller input, so truncation fails here.
    reason == Reason::ContextOverflow
}

/// Request failures that already used the model's bounded retry chain. The turn
/// continues without a summary while its retained input fits the window.
pub(super) fn transient(error: &adk_core::AdkError) -> bool {
    if error.category == adk_core::ErrorCategory::Cancelled {
        return false;
    }
    match classify(error) {
        (Stage::Request, Reason::RequestTimeout | Reason::RateLimited | Reason::RequestFailed) => {
            true
        }
        (Stage::Request, Reason::ServiceRejected) => error
            .details
            .upstream_status_code
            .is_none_or(|status| status >= 500 || status == 408),
        _ => false,
    }
}

pub(super) fn projectable(error: &adk_core::AdkError) -> bool {
    if error.category == adk_core::ErrorCategory::Cancelled {
        return false;
    }
    if let Some(reason) = request_reason(error) {
        return matches!(reason, Reason::ContextOverflow | Reason::TruncatedSummary);
    }
    matches!(
        classify(error).1,
        Reason::HistoryMismatch
            | Reason::NoFittingPrefix
            | Reason::TruncatedSummary
            | Reason::SummaryTooLarge
            | Reason::NoReduction
    )
}

pub(super) fn preserve(source: &adk_core::AdkError, target: &mut adk_core::AdkError) {
    if let Some(reason @ ("max_output_tokens" | "content_filter" | "interrupted")) = source
        .details
        .metadata
        .get("incomplete_reason")
        .and_then(serde_json::Value::as_str)
    {
        target
            .details
            .metadata
            .insert("incomplete_reason".into(), json!(reason));
    }
}

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
    fn separates_budget_and_terminal_reasons() {
        let incomplete = |reason: &str| {
            let mut error = adk_core::AdkError::model("incomplete response");
            error
                .details
                .metadata
                .insert("incomplete_reason".into(), json!(reason));
            error
        };
        let length = incomplete("max_output_tokens");
        assert_eq!(request_reason(&length), Some(Reason::TruncatedSummary));
        assert!(!recoverable(&length));
        let filtered = incomplete("content_filter");
        assert_eq!(request_reason(&filtered), Some(Reason::ContentFiltered));
        assert!(!recoverable(&filtered));
        for status in [401, 403, 429] {
            let error = incomplete("max_output_tokens").with_upstream_status(status);
            assert!(!recoverable(&error));
        }
        let overflow = adk_core::AdkError::new(
            adk_core::ErrorComponent::Model,
            adk_core::ErrorCategory::ResourceExhausted,
            "model.sailry.context_overflow",
            "retained context does not fit",
        );
        assert_eq!(request_reason(&overflow), Some(Reason::ContextOverflow));
        assert!(recoverable(&overflow));
        let mut safe = adk_core::AdkError::model("safe diagnostic");
        let mut source = length;
        source
            .details
            .metadata
            .insert("body".into(), json!("private request echo"));
        preserve(&source, &mut safe);
        assert_eq!(safe.details.metadata.len(), 1);
        assert_eq!(
            safe.details.metadata["incomplete_reason"],
            "max_output_tokens"
        );
    }

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
        for (category, reason, category_name) in [
            (ErrorCategory::Timeout, Reason::RequestTimeout, "timeout"),
            (
                ErrorCategory::RateLimited,
                Reason::RateLimited,
                "rate_limited",
            ),
            (
                ErrorCategory::Unauthorized,
                Reason::AuthorizationFailed,
                "unauthorized",
            ),
            (
                ErrorCategory::Unavailable,
                Reason::ServiceRejected,
                "unavailable",
            ),
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
            assert_eq!(
                transient(&safe),
                reason != Reason::AuthorizationFailed,
                "{category_name}"
            );
            let failure = Failure {
                source: Source::Automatic,
                stage: Stage::Request,
                reason,
                continued: true,
                status: Some(503),
                cause: cause(&safe),
            };
            let value = failure.resource();
            assert!(!value.to_string().contains("private response"));
            assert_eq!(
                value["failure"]["cause"],
                "The upstream service is temporarily unavailable"
            );
            assert_eq!(value["failure"]["status"], 503);
            let mut entry = crate::logging::Entry::new(
                crate::logging::Area::Model,
                crate::logging::Phase::Failed,
            );
            model::cause(&mut entry, &safe);
            assert_eq!(entry.cause_code, Some("fixture.private"));
            assert_eq!(entry.cause_component, Some("model"));
            assert_eq!(entry.cause_category, Some(category_name));
            assert_eq!(entry.upstream_status, Some(503));
        }
    }
}
