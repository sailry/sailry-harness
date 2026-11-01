//! Catalog commands run on the bound execution Node; outcomes surface only as toasts.
use super::Binding;
use crate::tr;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;
use sailry_protocol::{Command, ErrorCode, Fault, Output, Request};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Subject {
    Role,
    Team,
}

pub(super) type Job = tokio::task::JoinHandle<Result<Output, Fault>>;

pub(super) fn execute(binding: &Binding, request: Request) -> Job {
    let client = binding.client.clone();
    binding
        .runtime
        .spawn(async move { client.execute(request).await })
}

/// The request may have been applied; it is kept for an explicit retry with the same ID.
pub(super) fn uncertain(code: ErrorCode) -> bool {
    matches!(code, ErrorCode::OutcomeUnknown | ErrorCode::Unavailable)
}

pub(super) fn key(subject: Subject, code: ErrorCode) -> &'static str {
    match (subject, code) {
        (Subject::Role, ErrorCode::RevisionConflict | ErrorCode::NotFound) => "teams_role_conflict",
        (Subject::Team, ErrorCode::RevisionConflict | ErrorCode::NotFound) => "team_conflict",
        (Subject::Role, ErrorCode::Conflict) => "role_duplicate",
        (Subject::Team, ErrorCode::Conflict) => "team_name_taken",
        (Subject::Role, ErrorCode::InvalidRequest) => "teams_role_invalid",
        (Subject::Team, ErrorCode::InvalidRequest) => "team_invalid",
        (_, ErrorCode::NotConfigured) => "role_model_unavailable",
        (Subject::Role, ErrorCode::Busy) => "role_capacity",
        (_, code) if uncertain(code) => "role_unknown",
        _ => "role_failed",
    }
}

/// Removes one revision-checked record and reports the outcome; `after` runs on success.
pub(super) fn remove(
    binding: Binding,
    command: Command,
    subject: Subject,
    done: &'static str,
    window: &mut Window,
    cx: &mut App,
    after: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    let job = execute(&binding, binding.client.prepare(command));
    window
        .spawn(cx, async move |cx| {
            let result = job.await;
            let _ = cx.update(move |window, cx| match result {
                Ok(Ok(_)) => {
                    crate::feedback::toast(window, tr(done), Notification::success(tr(done)), cx);
                    after(window, cx);
                }
                Ok(Err(fault)) => {
                    let key = key(subject, fault.code);
                    crate::feedback::error(key, "", &tr(key), window, cx);
                }
                Err(_) => {
                    crate::feedback::error("role_unknown", "", &tr("role_unknown"), window, cx)
                }
            });
        })
        .detach();
}
