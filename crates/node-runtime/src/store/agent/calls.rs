//! Exact call association for authorization and user input.
use super::*;
use adk_core::ToolConfirmationRequest;

pub(super) fn active(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let run = runs::get(db, turn)?;
    if run.kind != RunKind::Task {
        return Err(invalid("context compaction cannot invoke tools"));
    }
    if run.status != Status::Running {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "tool invocation is no longer running",
        ));
    }
    Ok(run)
}

pub(super) fn id(request: &ToolConfirmationRequest) -> Result<&str, Fault> {
    request
        .function_call_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| invalid("tool call identifier is required"))
}

pub(super) fn matches(part: &adk_core::Part, request: &ToolConfirmationRequest) -> bool {
    matches!(part, adk_core::Part::FunctionCall { id, name, args, .. }
        if id == &request.function_call_id && name == &request.tool_name && args == &request.args)
}

pub(super) fn locate(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
) -> Result<(String, usize), Fault> {
    id(request)?;
    let mut query = db
        .prepare("SELECT body FROM agent_events WHERE turn=?1 ORDER BY sequence DESC")
        .map_err(storage_error)?;
    let rows = query
        .query_map([turn.to_string()], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?;
    let mut found = None;
    for body in rows {
        let event: AdkEvent =
            serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
        for (index, part) in event
            .content()
            .into_iter()
            .flat_map(|content| content.parts.iter().enumerate())
        {
            if let adk_core::Part::FunctionCall { id, .. } = part
                && id == &request.function_call_id
            {
                if !matches(part, request) || found.is_some() {
                    return Err(invalid(
                        "tool call identity is ambiguous or arguments differ",
                    ));
                }
                found = Some((event.id.clone(), index));
            }
        }
    }
    found.ok_or_else(|| invalid("request does not identify a canonical tool call"))
}
