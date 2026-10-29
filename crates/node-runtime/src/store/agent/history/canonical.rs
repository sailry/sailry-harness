//! Visible canonical events serve original model output and controller details.
use super::*;
use crate::agent::canonical::{BlockKind, Record};

pub(in crate::store::agent) struct Source {
    pub sequence: u64,
    pub turn: TurnId,
    pub event: Record,
}

pub(in crate::store::agent) async fn event(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    entry: &str,
) -> Result<Source, Fault> {
    super::super::runs::visible(db, session, turn).await?;
    if entry.is_empty() || entry.len() > 4096 {
        return Err(Fault::invalid("tool detail entry is invalid"));
    }
    let row = db
        .prepare(
            "SELECT e.sequence,e.turn,e.body FROM conversation_events h
         JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND e.turn=?2 AND e.id=?3 ORDER BY e.sequence DESC LIMIT 1",
        )
        .await
        .map_err(storage_error)?
        .query_row(params![session.to_string(), turn.to_string(), entry,])
        .await;
    match row {
        Ok(row) => source(&row),
        Err(turso::Error::QueryReturnedNoRows) => Err(Fault::new(
            ErrorCode::NotFound,
            "tool detail is not in this conversation",
        )),
        Err(error) => Err(storage_error(error)),
    }
}

/// Canonical records are immutable; membership and sequence validate cached details.
pub(in crate::store::agent) async fn sequence(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    entry: &str,
) -> Result<u64, Fault> {
    super::super::runs::visible(db, session, turn).await?;
    if entry.is_empty() || entry.len() > 4096 {
        return Err(Fault::invalid("tool detail entry is invalid"));
    }
    match db
        .prepare(
            "SELECT e.sequence FROM conversation_events h
         JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND e.turn=?2 AND e.id=?3 ORDER BY e.sequence DESC LIMIT 1",
        )
        .await
        .map_err(storage_error)?
        .query_row(params![session.to_string(), turn.to_string(), entry])
        .await
    {
        Ok(row) => Ok(row.get::<i64>(0).map_err(storage_error)? as u64),
        Err(turso::Error::QueryReturnedNoRows) => Err(Fault::new(
            ErrorCode::NotFound,
            "tool detail is not in this conversation",
        )),
        Err(error) => Err(storage_error(error)),
    }
}

pub(super) async fn result(
    db: &Connection,
    session: SessionId,
    reference: &sailry_protocol::tool::OutputReference,
) -> Result<(TurnId, Value), Fault> {
    let mut statement = db.prepare(
        "SELECT e.sequence,e.turn,e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
         WHERE h.session=?1 AND EXISTS(
             SELECT 1 FROM json_each(e.body,'$.content') part
             WHERE json_extract(part.value,'$.id')=?2 AND json_extract(part.value,'$.type')='tool_response')
         ORDER BY h.sequence DESC",
    ).await.map_err(storage_error)?;
    let mut rows = statement
        .query(params![session.to_string(), reference.call.as_str()])
        .await
        .map_err(storage_error)?;
    let mut found = false;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let source = source(&row)?;
        for block in source
            .event
            .message()
            .into_iter()
            .flat_map(|message| &message.content)
        {
            let BlockKind::ToolResponse(response) = &block.kind else {
                continue;
            };
            if response.id != reference.call {
                continue;
            }
            found = true;
            let value = super::super::blobs::hydrate(db, &response.result).await?;
            if blake3::hash(value.to_string().as_bytes()).to_hex().as_str() == reference.revision {
                return Ok((source.turn, value));
            }
        }
    }
    Err(if found {
        Fault::new(
            ErrorCode::RevisionConflict,
            "tool output reference does not match the visible result",
        )
    } else {
        Fault::new(
            ErrorCode::NotFound,
            "tool output is not in this conversation",
        )
    })
}

fn source(row: &turso::Row) -> Result<Source, Fault> {
    Ok(Source {
        sequence: row.get::<i64>(0).map_err(storage_error)? as u64,
        turn: row
            .get::<String>(1)
            .map_err(storage_error)?
            .parse()
            .map_err(storage_error)?,
        event: serde_json::from_slice(&row.get::<Vec<u8>>(2).map_err(storage_error)?)
            .map_err(storage_error)?,
    })
}
