//! Completion invalidation follows the canonical batch, not individual results.
use super::*;

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
         AND json_extract(body,'$.content.role')='model'
         AND EXISTS(SELECT 1 FROM json_each(e.body,'$.content.parts') p
             WHERE json_type(p.value,'$.name')='text' AND json_type(p.value,'$.args') IS NOT NULL)
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
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    let mut calls: Vec<_> = event
        .llm_response
        .content
        .into_iter()
        .flat_map(|content| content.parts)
        .filter_map(|part| match part {
            adk_core::Part::FunctionCall { name, id, .. } => Some((name, id)),
            _ => None,
        })
        .collect();
    let mut rows = db
        .query(
            "SELECT sequence,body FROM agent_events WHERE turn=?1 AND sequence>?2 AND sequence<=?3
         AND json_extract(body,'$.content.role')='function' ORDER BY sequence",
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
        let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
        for part in event
            .llm_response
            .content
            .into_iter()
            .flat_map(|content| content.parts)
        {
            if let adk_core::Part::FunctionResponse {
                function_response,
                id,
                ..
            } = part
                && let Some(index) = calls
                    .iter()
                    .position(|(name, call)| *name == function_response.name && *call == id)
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
