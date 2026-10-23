//! Shared labels and a deliberately bounded diagnostic report for notice surfaces.
#[cfg(test)]
mod tests;
use crate::tr;
use gpui_kit::SharedString;
use sailry_client::activity::{Kind, Notice, Target};
use sailry_protocol::{
    conversation::{Status, compaction::Source},
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

/// Preserve the original public text; classifications do not synthesize a body.
pub(super) fn content<'a>(notice: &'a Notice) -> Vec<&'a str> {
    let mut values = Vec::new();
    let mut push = |value: &'a str| {
        if !value.trim().is_empty() && !values.contains(&value) {
            values.push(value);
        }
    };
    push(&notice.message);
    match &notice.details {
        Some(Details::Text { body }) => push(body),
        Some(Details::Diagnostic(diagnostic)) => {
            if let Cause::Compaction(failure) = &diagnostic.cause
                && let Some(cause) = &failure.cause
            {
                push(cause);
            }
        }
        _ => (),
    }
    if let Some(error) = &notice.error {
        push(&error.message);
    }
    values
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
