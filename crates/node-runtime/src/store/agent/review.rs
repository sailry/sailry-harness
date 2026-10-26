//! Completed plans use canonical metadata, not fabricated provider tool calls.
use super::*;
use adk_core::ToolConfirmationRequest;
use question::{Answer, Response, Spec};
use serde::{Deserialize, Serialize};

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

pub(crate) fn event(turn: TurnId, source: String) -> AdkEvent {
    let mut event = AdkEvent::new(turn.to_string());
    event.author = TOOL.into();
    event.provider_metadata.insert(SOURCE.into(), source);
    event
}

pub(crate) fn result(turn: TurnId, call: String, response: Response) -> AdkEvent {
    let mut event = AdkEvent::new(turn.to_string());
    event.author = TOOL.into();
    event.provider_metadata.insert(
        RESULT.into(),
        serde_json::to_string(&Recorded { call, response }).expect("plan response is serializable"),
    );
    event
}

pub(super) fn spec(event: &AdkEvent) -> Option<Spec> {
    event.provider_metadata.contains_key(SOURCE).then(|| Spec {
        prompt: String::new(),
        input: question::Input::Plan,
    })
}

pub(super) async fn validate(db: &Connection, turn: TurnId, event: &AdkEvent) -> Result<(), Fault> {
    let Some(source) = event.provider_metadata.get(SOURCE) else {
        return Ok(());
    };
    let run = runs::get(db, turn).await?;
    if event.content().is_some()
        || event.author != TOOL
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
    let source: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    if source.llm_response.partial
        || source.llm_response.error_code.is_some()
        || source.llm_response.error_message.is_some()
        || !source.is_final_response()
        || source
            .content()
            .is_none_or(|content| content.role != "model")
    {
        return Err(sailry_protocol::Fault::invalid(
            "plan review has no completed answer",
        ));
    }
    Ok(())
}

pub(super) async fn locate(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
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
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    if request.tool_name != TOOL
        || spec(&event).is_none_or(|spec| {
            serde_json::to_value(spec).expect("plan input is serializable") != request.args
        })
    {
        return Err(sailry_protocol::Fault::invalid(
            "plan review arguments differ",
        ));
    }
    Ok((call.into(), 0))
}

pub(super) fn project(event: &AdkEvent) -> Result<Vec<history::tools::Raw>, Fault> {
    if let Some(spec) = spec(event) {
        return Ok(vec![history::tools::Raw::new(
            Some(event.id.clone()),
            TOOL.into(),
            serde_json::to_value(spec).map_err(storage_error)?,
        )]);
    }
    let Some(recorded) = event.provider_metadata.get(RESULT) else {
        return Ok(Vec::new());
    };
    let Recorded { call, response } = serde_json::from_str(recorded).map_err(storage_error)?;
    let result = match response {
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
    let mut raw = history::tools::Raw::new(Some(call), TOOL.into(), Value::Null);
    raw.result = Some(result);
    Ok(vec![raw])
}
