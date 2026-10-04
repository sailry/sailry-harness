//! Session visibility is durable Node state. Removed sessions retain immutable
//! history needed by forks and usage, but cannot be opened or resumed.
use super::*;
use crate::store::database::encode;

pub(in crate::store) fn archive(
    db: &Connection,
    id: SessionId,
    expected_revision: u64,
    archived: bool,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = commands::session(db, id)?;
    writable(&session)?;
    commands::check_revision(session.revision, expected_revision)?;
    session.archived = archived;
    db.execute(
        "UPDATE sessions SET state=?2 WHERE id=?1",
        params![id.to_string(), if archived { "archived" } else { "active" }],
    )
    .map_err(storage_error)?;
    commands::revise_session(db, session)
}

pub(in crate::store) fn remove(
    db: &Connection,
    id: SessionId,
    expected_revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let session = commands::session(db, id)?;
    writable(&session)?;
    commands::check_revision(session.revision, expected_revision)?;
    let mut query = db
        .prepare(
            "WITH RECURSIVE owned(id) AS (
            SELECT ?1 UNION SELECT d.session FROM session_delegations d
            JOIN turns t ON t.id=d.parent_turn JOIN owned ON owned.id=t.session
        ) SELECT id FROM owned",
        )
        .map_err(storage_error)?;
    let ids = query
        .query_map([id.to_string()], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
        .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
        .collect::<Result<Vec<SessionId>, Fault>>()?;
    for id in &ids {
        let busy: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn
             WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))",
            [id.to_string()], |row| row.get(0)
        ).map_err(storage_error)?;
        if busy {
            return Err(Fault::new(
                ErrorCode::Busy,
                "stop the session before deleting it",
            ));
        }
    }
    for id in &ids {
        db.execute(
            "UPDATE sessions SET state='removed' WHERE id=?1",
            [id.to_string()],
        )
        .map_err(storage_error)?;
    }
    Ok((
        Output::SessionsRemoved(ids.clone()),
        Some(Event::SessionsRemoved(ids)),
    ))
}

/// Rename the same session title used by the agent naming tool.
pub(in crate::store) fn rename(
    db: &Connection,
    id: SessionId,
    expected_revision: u64,
    title: &str,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = commands::session(db, id)?;
    writable(&session)?;
    commands::check_revision(session.revision, expected_revision)?;
    let title = title.trim();
    if title.is_empty() || title.chars().any(char::is_control) {
        return Err(commands::invalid(
            "session title must be nonempty and contain no control characters",
        ));
    }
    db.execute(
        "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body",
        params![id.to_string(), crate::store::agent::SESSION_TITLE, encode(&title)?],
    )
    .map_err(storage_error)?;
    session.activity.title = crate::store::agent::activity::title(db, id)?;
    commands::revise_session(db, session)
}
