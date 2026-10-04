//! Compact session labels shared by native resource surfaces.
use super::*;
use sailry_protocol::{Session, activity::Waiting, conversation::Status};

pub(crate) fn title(session: &Session) -> SharedString {
    let title = session.activity.title.trim();
    if title.is_empty() {
        tr("chat_new")
    } else {
        title.replace(['\n', '\r'], " ").into()
    }
}

pub(crate) fn status(session: &Session) -> SharedString {
    if session.activity.queued > 0
        && session
            .activity
            .run
            .as_ref()
            .is_none_or(|run| !matches!(run.status, Status::Running | Status::Stopping))
    {
        return format!("{} {}", tr("turn_queued"), session.activity.queued).into();
    }
    let key = match session.activity.run.as_ref().map(|run| run.status) {
        Some(Status::Queued) => "turn_queued",
        Some(Status::Stopping) => "chat_stopping",
        Some(Status::Running) => match session.activity.waiting {
            Some(Waiting::Approval) => "activity_approval",
            Some(Waiting::Input) => "activity_input",
            None => "activity_running",
        },
        Some(Status::Completed) => "activity_completed",
        Some(Status::Failed) => "activity_failed",
        Some(Status::Interrupted) => "chat_interrupted",
        Some(Status::Cancelled) => "turn_cancelled",
        None => "activity_idle",
    };
    if session.activity.queued > 0 {
        format!(
            "{} · {} {}",
            tr(key),
            tr("turn_queued"),
            session.activity.queued
        )
        .into()
    } else {
        tr(key)
    }
}
