//! Completion invalidation follows the canonical batch, not individual results.
use super::*;
use crate::agent::canonical::{BlockKind, Record};

pub(super) async fn completed(db: &Connection, turn: TurnId, entry: &Entry) -> Result<bool, Fault> {
    if !entry
        .parts
        .iter()
        .any(|part| matches!(part, Part::ToolResult { .. }))
    {
        return Ok(false);
    }
    let batch = db
        .prepare(
            "SELECT sequence,body FROM agent_events e WHERE turn=?1 AND sequence<?2
         AND json_extract(body,'$.role')='assistant'
         AND EXISTS(SELECT 1 FROM json_each(e.body,'$.content') p
             WHERE json_extract(p.value,'$.type')='tool_request')
         ORDER BY sequence DESC LIMIT 1",
        )
        .await
        .map_err(storage_error)?
        .query_row(params![turn.to_string(), entry.sequence as i64])
        .await;
    let (sequence, body): (i64, Vec<u8>) = match batch {
        Ok(row) => (
            row.get(0).map_err(storage_error)?,
            row.get(1).map_err(storage_error)?,
        ),
        Err(turso::Error::QueryReturnedNoRows) => return Ok(false),
        Err(error) => return Err(storage_error(error)),
    };
    let event: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    let mut calls: Vec<_> = event
        .message()
        .into_iter()
        .flat_map(|message| &message.content)
        .filter_map(|block| match &block.kind {
            BlockKind::ToolRequest(request) => Some((request.name.clone(), request.id.clone())),
            _ => None,
        })
        .collect();
    let mut rows = db
        .query(
            "SELECT sequence,body FROM agent_events WHERE turn=?1 AND sequence>?2 AND sequence<=?3
         AND EXISTS(SELECT 1 FROM json_each(body,'$.content') part WHERE json_extract(part.value,'$.type')='tool_response') ORDER BY sequence",
            params![turn.to_string(), sequence, entry.sequence as i64],
        )
        .await
        .map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        if calls.is_empty() {
            return Ok(false);
        }
        let sequence: i64 = row.get(0).map_err(storage_error)?;
        let body: Vec<u8> = row.get(1).map_err(storage_error)?;
        let event: Record = serde_json::from_slice(&body).map_err(storage_error)?;
        for block in event
            .message()
            .into_iter()
            .flat_map(|message| &message.content)
        {
            if let BlockKind::ToolResponse(response) = &block.kind
                && let Some(index) = calls
                    .iter()
                    .position(|(name, call)| *name == response.name && *call == response.id)
            {
                calls.remove(index);
            }
        }
        if sequence == entry.sequence as i64 {
            return Ok(calls.is_empty());
        }
    }
    Ok(false)
}
