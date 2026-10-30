//! Terminal turns are counted once, after the canonical run commit.
use crate::telemetry::metrics::{Observation, TurnResult};
use sailry_protocol::{
    ErrorCode, WorkMode,
    conversation::{Run, RunKind, Status},
};
use turso::{Connection, params};

pub(super) async fn turn(db: &Connection, run: &Run, subagent: bool) -> Option<Observation> {
    if run.kind != RunKind::Task {
        return None;
    }
    let result = match run.status {
        Status::Completed | Status::Continued => TurnResult::Completed,
        Status::Failed | Status::Interrupted => TurnResult::Failed,
        Status::Cancelled => TurnResult::Cancelled,
        _ => return None,
    };
    let mode: String = db.prepare("SELECT json_extract(config,'$.mode') FROM session_revisions WHERE session=?1 AND revision=?2")
        .await.ok()?.query_row(params![run.session.to_string(),run.revision as i64])
        .await.ok()?.get(0).ok()?;
    let mode = serde_json::from_value::<WorkMode>(serde_json::Value::String(mode)).ok()?;
    Some(Observation::Turn {
        result,
        mode,
        subagent,
        error: run
            .error
            .as_ref()
            .map(|error| error.code)
            .or((result == TurnResult::Cancelled).then_some(ErrorCode::Cancelled)),
    })
}
