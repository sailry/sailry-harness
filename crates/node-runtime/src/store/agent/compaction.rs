//! Derive model context from canonical membership, never wall-clock ordering.
use super::*;
use crate::agent::canonical::{Kind, Record};

pub(crate) const END_EVENT: &str = "sailry.context.end_event";

pub(super) async fn validate(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    event: &Record,
) -> Result<(), Fault> {
    let Kind::Compaction(summary) = &event.kind else {
        return Ok(());
    };
    let valid=db.query("SELECT 1 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.id=?2",params![session.to_string(),summary.boundary.as_str()]).await.map_err(storage_error)?.next().await.map_err(storage_error)?.is_some();
    if !valid {
        return Err(Fault::invalid(
            "compaction boundary does not belong to this session",
        ));
    }
    let _ = turn;
    Ok(())
}

pub(super) async fn read(
    db: &Connection,
    session: SessionId,
) -> Result<Option<(i64, Record)>, Fault> {
    let mut rows = db.query("SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND json_extract(e.body,'$.kind')='compaction' ORDER BY h.sequence DESC LIMIT 1",[session.to_string()]).await.map_err(storage_error)?;
    let Some(row) = rows.next().await.map_err(storage_error)? else {
        return Ok(None);
    };
    let body: Vec<u8> = row.get(0).map_err(storage_error)?;
    let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    let Kind::Compaction(summary) = &record.kind else {
        unreachable!("selected compaction record");
    };
    let mut rows = db.query("SELECT h.sequence FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.id=?2", params![session.to_string(),summary.boundary.as_str()]).await.map_err(storage_error)?;
    let row = rows
        .next()
        .await
        .map_err(storage_error)?
        .ok_or_else(|| Fault::invalid("compaction boundary is missing"))?;
    Ok(Some((row.get(0).map_err(storage_error)?, record)))
}
