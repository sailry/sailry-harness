//! Exact call association for authorization and user input.
use super::*;
use crate::agent::canonical::ToolIdentity;

pub(super) async fn active(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let run = runs::get(db, turn).await?;
    if run.kind != RunKind::Task {
        return Err(sailry_protocol::Fault::invalid(
            "context compaction cannot invoke tools",
        ));
    }
    if run.status != Status::Running {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "tool invocation is no longer running",
        ));
    }
    Ok(run)
}

pub(super) fn id(request: &ToolIdentity) -> Result<&str, Fault> {
    request
        .id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| sailry_protocol::Fault::invalid("tool call identifier is required"))
}

pub(super) fn matches(part: &crate::agent::canonical::BlockKind, request: &ToolIdentity) -> bool {
    matches!(part, crate::agent::canonical::BlockKind::ToolRequest(call)
        if call.id == request.id && call.name == request.name && call.arguments == request.arguments)
}

pub(super) async fn locate(
    db: &Connection,
    turn: TurnId,
    request: &ToolIdentity,
) -> Result<(String, usize), Fault> {
    id(request)?;
    let mut query = db
        .prepare("SELECT body FROM agent_events WHERE turn=?1 ORDER BY sequence DESC")
        .await
        .map_err(storage_error)?;
    let rows = {
        let mut rows = query
            .query([turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        values.into_iter()
    };
    let mut found = None;
    for body in rows {
        let event: crate::agent::canonical::Record =
            serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
        for block in event
            .message()
            .into_iter()
            .flat_map(|message| &message.content)
        {
            if let crate::agent::canonical::BlockKind::ToolRequest(call) = &block.kind
                && call.id == request.id
            {
                if !matches(&block.kind, request) || found.is_some() {
                    return Err(sailry_protocol::Fault::invalid(
                        "tool call identity is ambiguous or arguments differ",
                    ));
                }
                found = Some((event.id.clone(), block.index));
            }
        }
    }
    found.ok_or_else(|| {
        sailry_protocol::Fault::invalid("request does not identify a canonical tool call")
    })
}
