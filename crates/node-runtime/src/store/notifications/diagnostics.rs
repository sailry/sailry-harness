//! One safe system report is persisted for each source failure cycle.
use super::*;
use sailry_protocol::notification::{Details, DiagnosticReason, Draft, Kind, SystemDiagnostic};

pub(in crate::store) async fn report(
    db: &Connection,
    report: &SystemDiagnostic,
) -> Result<(Output, Option<Event>), Fault> {
    if !report.valid() {
        return Err(Fault::invalid("invalid system diagnostic"));
    }
    if report.code == ErrorCode::Cancelled
        || (report.reason == DiagnosticReason::Cancelled
            && report.code != ErrorCode::OutcomeUnknown)
    {
        return Ok((Output::NotificationsUpdated, None));
    }
    let session = (match report.session {
        Some(value) => {
            Some((async |id| super::super::commands::session(db, id).await)(value).await)
        }
        None => None,
    })
    .transpose()?;
    if let Some(worktree) = report.worktree {
        if !super::super::worktrees::list(db)
            .await?
            .iter()
            .any(|entry| entry.id == worktree)
        {
            return Err(Fault::new(
                ErrorCode::NotFound,
                "diagnostic worktree is unavailable",
            ));
        }
        if session
            .as_ref()
            .is_some_and(|session| session.worktree != worktree)
        {
            return Err(Fault::invalid("diagnostic resources do not share an owner"));
        }
    }
    if let Some(turn) = report.turn {
        let run = super::super::agent::runs::get(db, turn).await?;
        if report.session != Some(run.session) {
            return Err(Fault::invalid("diagnostic turn belongs to another session"));
        }
    }
    // Also deduplicate independent controller requests carrying the same incident.
    if let Some(notice) = list(db).await?.into_iter().find(|notice| {
        matches!(&notice.content.details,
        Some(Details::System(existing)) if existing.incident == report.incident)
    }) {
        return Ok((Output::Notification(notice), None));
    }
    let notice = insert(
        db,
        String::new(),
        Draft {
            title: String::new(),
            message: String::new(),
            kind: if report.code == ErrorCode::OutcomeUnknown {
                Kind::Warning
            } else {
                Kind::Error
            },
            session: report.session,
            presentation: None,
            details: Some(Details::System(report.clone())),
        },
        chrono::Utc::now().timestamp_millis(),
    )
    .await?;
    emit(report);
    Ok((
        Output::Notification(notice.clone()),
        Some(Event::NotificationChanged(notice)),
    ))
}

fn emit(report: &SystemDiagnostic) {
    fn token(value: impl serde::Serialize) -> String {
        serde_json::to_value(value)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }
    tracing::warn!(target: "sailry", area = "command", phase = "failed",
        incident = report.incident.to_string().as_str(),
        request = report.request.map(|id| id.to_string()).unwrap_or_default().as_str(),
        source_kind = token(report.source).as_str(), stage = token(report.phase).as_str(),
        reason = token(report.reason).as_str(), code = token(report.code).as_str(),
        exception = report.exception.map(token).unwrap_or_default().as_str(),
        operation = report.operation.as_deref().unwrap_or_default(),
        package = report.plugin.as_ref().map_or("", |plugin| plugin.name.as_str()),
        plugin_version = report.plugin.as_ref().map_or("", |plugin| plugin.version.as_str()),
        session = report.session.map(|id| id.to_string()).unwrap_or_default().as_str(),
        turn = report.turn.map(|id| id.to_string()).unwrap_or_default().as_str(),
        worktree = report.worktree.map(|id| id.to_string()).unwrap_or_default().as_str(),
        budget = report.budget_ms, elapsed_us = report.elapsed_ms.map(|value| value.saturating_mul(1000)),
        app_version = report.version.as_str(), platform = report.platform.as_str());
}
