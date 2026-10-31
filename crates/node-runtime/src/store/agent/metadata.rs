//! Read lifecycle fields directly; no canonical events, captures or notice bodies are hydrated.
use super::*;
use sailry_protocol::conversation::runs::{Page, Summary};

#[cfg(test)]
mod tests;

const FIELDS: &str = "json_object('turn',a.turn,'sequence',t.rowid,
    'status',json_extract(a.body,'$.status'),
    'origin',CASE WHEN t.session!=?1 THEN t.session ELSE json_extract(a.body,'$.origin') END,
    'error_code',json_extract(a.body,'$.error.code'))";

pub(in crate::store) async fn page(
    db: &Connection,
    session: SessionId,
    before: Option<TurnId>,
    limit: u16,
) -> Result<Page, Fault> {
    if limit == 0 {
        return Err(Fault::invalid("run page limit must be positive"));
    }
    let revision = crate::store::sessions::history_revision(db, session).await?;
    let boundary = match before {
        Some(turn) => read(db, session, turn, Some(revision)).await?.sequence,
        None => i64::MAX as u64,
    };
    let limit = limit.min(100);
    let sql = format!(
        "SELECT {FIELDS} FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=t.id WHERE h.session=?1 AND t.rowid<?2 ORDER BY t.rowid DESC LIMIT ?3"
    );
    let mut rows = db
        .query(
            &sql,
            params![session.to_string(), boundary as i64, i64::from(limit) + 1],
        )
        .await
        .map_err(storage_error)?;
    let mut runs = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        runs.push(decode(&row)?);
    }
    let more = runs.len() > usize::from(limit);
    runs.truncate(usize::from(limit));
    let next_before = more.then(|| runs.last().expect("positive bounded page").turn);
    runs.reverse();
    let queue_revision = db
        .prepare("SELECT coalesce((SELECT revision FROM agent_queues WHERE session=?1),0)")
        .await
        .map_err(storage_error)?
        .query_row([session.to_string()])
        .await
        .and_then(|row| row.get::<i64>(0))
        .map_err(storage_error)? as u64;
    Ok(Page {
        session,
        revision,
        runs,
        next_before,
        queue_revision,
    })
}

pub(in crate::store) async fn read(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    expected_revision: Option<u64>,
) -> Result<Summary, Fault> {
    let revision = crate::store::sessions::history_revision(db, session).await?;
    if let Some(expected) = expected_revision {
        crate::store::commands::check_revision(revision, expected)?;
    }
    let sql = format!(
        "SELECT {FIELDS} FROM conversation_turns h JOIN turns t ON t.id=h.turn JOIN agent_runs a ON a.turn=t.id WHERE h.session=?1 AND h.turn=?2"
    );
    let row = db
        .prepare(&sql)
        .await
        .map_err(storage_error)?
        .query_row(params![session.to_string(), turn.to_string()])
        .await;
    match row {
        Ok(row) => decode(&row),
        Err(turso::Error::QueryReturnedNoRows) => Err(Fault::new(
            ErrorCode::NotFound,
            "turn is not in this conversation",
        )),
        Err(error) => Err(storage_error(error)),
    }
}

fn decode(row: &turso::Row) -> Result<Summary, Fault> {
    serde_json::from_str(&row.get::<String>(0).map_err(storage_error)?).map_err(storage_error)
}
