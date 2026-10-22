//! Shared labels and a deliberately bounded diagnostic report for notice surfaces.
#[cfg(test)]
mod tests;
use crate::tr;
use gpui_kit::SharedString;
use sailry_client::activity::{Kind, Notice, Target};
use sailry_protocol::{
    conversation::{
        Status,
        compaction::{Failure, Reason, Source, Stage},
    },
    notification::{Cause, Details},
};

pub(super) fn heading(notice: &Notice) -> SharedString {
    if let Some(Details::System(report)) = &notice.details {
        return tr(system_key(report));
    }
    if matches!(notice.details, Some(Details::Power { .. })) {
        return tr("power_request_failed");
    }
    if let Some(Details::Diagnostic(diagnostic)) = &notice.details {
        return match &diagnostic.cause {
            Cause::Compaction(failure) => tr(if failure.source == Source::Automatic {
                "compact_auto_failed"
            } else {
                "chat_compact_failed"
            }),
            Cause::Execution { .. } => tr("activity_failed"),
        };
    }
    if !notice.title.trim().is_empty() {
        return notice.title.clone().into();
    }
    tr(match notice.target() {
        Target::Session(_) => "conversation",
        Target::Terminal(_) => "terminal",
        Target::Notification(_) => "notifications",
    })
}

pub(super) fn label(notice: &Notice) -> SharedString {
    tr(match notice.kind {
        Kind::Completed => "activity_completed",
        Kind::Failed => "activity_failed",
        Kind::Approval => "activity_approval",
        Kind::Input => "activity_input",
        Kind::Notification(_) => "notifications",
    })
}

pub(super) fn status(status: Status) -> SharedString {
    tr(match status {
        Status::Queued => "turn_queued",
        Status::Running => "activity_running",
        Status::Stopping => "chat_stopping",
        Status::Completed => "activity_completed",
        Status::Continued => "turn_continued",
        Status::Cancelled => "turn_cancelled",
        Status::Interrupted => "chat_interrupted",
        Status::Failed => "activity_failed",
    })
}

pub(super) fn failure(failure: &Failure) -> Vec<SharedString> {
    let stage = match failure.stage {
        Stage::History => "compact_stage_history",
        Stage::Budget => "compact_stage_budget",
        Stage::Request => "compact_stage_request",
        Stage::Summary => "compact_stage_summary",
        Stage::Persistence => "compact_stage_persistence",
        Stage::Unknown => "compact_stage_unknown",
    };
    let (reason, suggestion) = match failure.reason {
        Reason::HistoryMismatch => ("compact_reason_history", "compact_suggest_report"),
        Reason::NoFittingPrefix => ("compact_reason_budget", "compact_suggest_context"),
        Reason::RequestTimeout => ("compact_reason_timeout", "compact_suggest_retry"),
        Reason::RateLimited => ("compact_reason_rate", "compact_suggest_wait"),
        Reason::AuthorizationFailed => ("compact_reason_auth", "compact_suggest_auth"),
        Reason::ServiceRejected => ("compact_reason_rejected", "compact_suggest_retry"),
        Reason::EmptySummary => ("compact_reason_empty", "compact_suggest_retry"),
        Reason::RequestFailed => ("compact_reason_request", "compact_suggest_retry"),
        Reason::TruncatedSummary => ("compact_reason_truncated", "compact_suggest_context"),
        Reason::IncompleteResponse => ("compact_reason_incomplete", "compact_suggest_retry"),
        Reason::UnexpectedOutput => ("compact_reason_output", "compact_suggest_retry"),
        Reason::SummaryTooLarge => ("compact_reason_size", "compact_suggest_context"),
        Reason::NoReduction => ("compact_reason_reduction", "compact_suggest_context"),
        Reason::PersistenceFailed => ("compact_reason_persistence", "compact_suggest_retry"),
        Reason::Unknown => ("compact_reason_unknown", "compact_suggest_retry"),
    };
    let mut lines = vec![tr(stage), tr(reason)];
    if let Some(status) = failure.status {
        lines.push(format!("HTTP {status}").into());
    }
    lines.push(tr(if failure.continued {
        "compact_task_continued"
    } else {
        "compact_task_stopped"
    }));
    lines.push(tr(suggestion));
    lines
}

pub(super) fn timestamp(milliseconds: i64) -> SharedString {
    chrono::DateTime::from_timestamp_millis(milliseconds)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
        .into()
}

pub(super) fn report(notice: &Notice) -> Option<String> {
    let details = notice.details.as_ref()?;
    if matches!(details, Details::Text { .. }) {
        return None;
    }
    // Exclude title, message, plugin-authored text, and raw model/provider errors.
    serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "notification": notice.id,
        "timestamp_ms": notice.timestamp_ms,
        "session": notice.session,
        "controller": {
            "version": env!("CARGO_PKG_VERSION"),
            "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        },
        "details": details,
    }))
    .ok()
}

/// Unknown side effects take precedence over a timeout explanation.
pub(crate) fn system_key(report: &sailry_protocol::notification::SystemDiagnostic) -> &'static str {
    use sailry_protocol::{
        ErrorCode,
        notification::{DiagnosticReason, SystemSource},
    };
    if report.code == ErrorCode::OutcomeUnknown {
        return "chat_execution_unknown";
    }
    if report.reason == DiagnosticReason::Cancelled || report.code == ErrorCode::Cancelled {
        return "turn_cancelled";
    }
    if report.reason == DiagnosticReason::Timeout {
        return "plugin_response_timeout";
    }
    match report.source {
        SystemSource::PluginMetadata => "plugin_metadata_failed",
        SystemSource::PluginInitialization => "plugin_initialization_failed",
        SystemSource::GitInspection => "git_inspection_failed",
        SystemSource::PluginCallback => "plugin_callback_failed",
    }
}
