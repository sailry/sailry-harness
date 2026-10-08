//! Completed plans use canonical metadata, not fabricated provider tool calls.
use super::*;
use adk_core::ToolConfirmationRequest;
use question::{Answer, Response, Spec};
use serde::{Deserialize, Serialize};

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

pub(super) fn validate(db: &Connection, turn: TurnId, event: &AdkEvent) -> Result<(), Fault> {
    let Some(source) = event.provider_metadata.get(SOURCE) else {
        return Ok(());
    };
    let run = calls::active(db, turn)?;
    if event.content().is_some()
        || event.author != TOOL
        || permissions::config(db, turn)?.mode != WorkMode::Plan
    {
        return Err(sailry_protocol::Fault::invalid(
            "plan review requires a planning completion",
        ));
    }
    crate::store::sessions::writable(&crate::store::commands::session(db, run.session)?)?;
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![turn.to_string(), source],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let source: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    if source.llm_response.partial
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

pub(super) fn locate(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
) -> Result<(String, usize), Fault> {
    let call = calls::id(request)?;
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![turn.to_string(), call],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
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

pub(super) fn project(event: &AdkEvent) -> Result<Vec<Part>, Fault> {
    if let Some(spec) = spec(event) {
        return Ok(vec![Part::ToolCall {
            display: None,
            presentation: sailry_protocol::tool::Presentation::Summary,
            grouping: Default::default(),
            id: Some(event.id.clone()),
            name: TOOL.into(),
            arguments: serde_json::to_value(spec).map_err(storage_error)?,
        }]);
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
    Ok(vec![Part::ToolResult {
        id: Some(call),
        name: TOOL.into(),
        result,
        images: Vec::new(),
    }])
}
