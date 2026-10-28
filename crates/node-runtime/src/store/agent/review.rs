//! Completed plans use canonical metadata, not fabricated provider tool calls.
use super::*;
use crate::agent::canonical::ToolIdentity;
use crate::agent::canonical::{BlockKind, Finish, Kind, Notice, Record, Role};
use question::{Answer, Response, Spec};
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod deferred;
pub(in crate::store) use deferred::{Changes, command};
pub(super) use deferred::{completed, publish, supersede};

const SOURCE: &str = "sailry.plan.review";
const RESULT: &str = "sailry.plan.review_result";
pub(crate) const TOOL: &str = "plan_review";

#[derive(Serialize, Deserialize)]
struct Recorded {
    call: String,
    response: Response,
}

pub(crate) fn event(turn: TurnId, source: String) -> Record {
    let mut event = Record::new(turn, Kind::Notice(Notice::PlanReview { source }));
    event.provenance.author = TOOL.into();
    event
}

pub(crate) fn result(turn: TurnId, call: String, response: Response) -> Record {
    let mut event = Record::new(turn, Kind::Notice(Notice::PlanResult { call, response }));
    event.provenance.author = TOOL.into();
    event
}

pub(super) fn spec(event: &Record) -> Option<Spec> {
    matches!(&event.kind, Kind::Notice(Notice::PlanReview { .. })).then(|| Spec {
        prompt: String::new(),
        input: question::Input::Plan,
    })
}

pub(super) async fn validate(db: &Connection, turn: TurnId, event: &Record) -> Result<(), Fault> {
    let Kind::Notice(Notice::PlanReview { source }) = &event.kind else {
        return Ok(());
    };
    let run = runs::get(db, turn).await?;
    if event.provenance.author != TOOL
        || run.kind != RunKind::Task
        || !matches!(run.status, Status::Running | Status::Completed)
        || permissions::config(db, turn).await?.mode != WorkMode::Plan
    {
        return Err(sailry_protocol::Fault::invalid(
            "plan review requires a planning completion",
        ));
    }
    let session = crate::store::commands::session(db, run.session).await?;
    crate::store::sessions::writable(&session)?;
    if session.delegation.is_some() {
        return Err(sailry_protocol::Fault::invalid(
            "child plans do not require review",
        ));
    }
    let body: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![turn.to_string(), source.as_str()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    let source: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    if source.message().is_none_or(|message| {
        message.error.is_some()
            || matches!(message.finish, Some(Finish::Interrupted | Finish::Failed))
            || message.role != Role::Assistant
            || message
                .content
                .iter()
                .any(|block| matches!(block.kind, BlockKind::ToolRequest(_)))
    }) {
        return Err(sailry_protocol::Fault::invalid(
            "plan review has no completed answer",
        ));
    }
    Ok(())
}

pub(super) async fn locate(
    db: &Connection,
    turn: TurnId,
    request: &ToolIdentity,
) -> Result<(String, usize), Fault> {
    let call = calls::id(request)?;
    let body: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![turn.to_string(), call])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    let event: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    if request.name != TOOL
        || spec(&event).is_none_or(|spec| {
            serde_json::to_value(spec).expect("plan input is serializable") != request.arguments
        })
    {
        return Err(sailry_protocol::Fault::invalid(
            "plan review arguments differ",
        ));
    }
    Ok((call.into(), 0))
}

pub(super) fn project(event: &Record) -> Result<Vec<history::tools::Raw>, Fault> {
    if let Some(spec) = spec(event) {
        return Ok(vec![history::tools::Raw::new(
            Some(event.id.clone()),
            TOOL.into(),
            serde_json::to_value(spec).map_err(storage_error)?,
        )]);
    }
    let Kind::Notice(Notice::PlanResult { call, response }) = &event.kind else {
        return Ok(Vec::new());
    };
    let result = match response.clone() {
        Response::Answer(Answer::Text(text)) => {
            serde_json::json!({"status": "answered", "answer": text})
        }
        Response::Answer(Answer::Plan { turn }) => {
            serde_json::json!({"status": "accepted", "coding_turn": turn})
        }
        Response::Cancel => serde_json::json!({"status": "cancelled"}),
        _ => {
            return Err(sailry_protocol::Fault::invalid(
                "plan review has an invalid response",
            ));
        }
    };
    let mut raw = history::tools::Raw::new(Some(call.clone()), TOOL.into(), Value::Null);
    raw.result = Some(result);
    Ok(vec![raw])
}
