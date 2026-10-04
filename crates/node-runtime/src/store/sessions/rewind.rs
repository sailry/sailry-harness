//! Rewinds change visible membership, never canonical history or external side effects.
use super::*;
use crate::store::agent;
use sailry_protocol::conversation::Rewind;

pub(in crate::store) fn revision(db: &Connection, session: SessionId) -> Result<u64, Fault> {
    db.query_row(
        "SELECT history_revision FROM sessions WHERE id=?1",
        [session.to_string()],
        |row| row.get::<_, i64>(0).map(|revision| revision as u64),
    )
    .optional()
    .map_err(storage_error)?
    .ok_or_else(|| Fault::new(ErrorCode::NotFound, "session does not exist"))
}

pub(in crate::store) fn rewind(
    db: &Connection,
    session: SessionId,
    through: Option<TurnId>,
    expected_head: TurnId,
    expected_revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let current = commands::session(db, session)?;
    writable(&current)?;
    let revision = revision(db, session)?;
    commands::check_revision(revision, expected_revision)?;
    let busy: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation_turns h JOIN agent_runs a ON a.turn=h.turn
             WHERE h.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))",
            [session.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if busy {
        return Err(Fault::new(
            ErrorCode::Busy,
            "conversation has active or queued turns",
        ));
    }
    let head: Option<String> = db
        .query_row(
            "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn
             WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1",
            [session.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if head != Some(expected_head.to_string()) {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "conversation head changed",
        ));
    }
    if through == Some(expected_head) {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "conversation is already at this turn",
        ));
    }
    let boundary = through
        .map(|turn| agent::visible_run(db, session, turn))
        .transpose()?;
    let (Output::Session(backup), _) =
        super::forks::backup(db, session, expected_head, current.revision)?
    else {
        unreachable!("history backup returns a session")
    };
    db.execute(
        "DELETE FROM conversation_turns WHERE session=?1 AND turn IN
         (SELECT id FROM turns WHERE rowid>?2)",
        params![
            session.to_string(),
            boundary.map_or(0, |run| run.sequence) as i64
        ],
    )
    .map_err(storage_error)?;
    db.execute(
        "UPDATE sessions SET history_revision=history_revision+1 WHERE id=?1",
        [session.to_string()],
    )
    .map_err(storage_error)?;
    agent::rebuild_event_index(db, session)?;
    agent::rebuild_session_state(db, session)?;
    crate::store::plugins::storage::conversation::rewind(db, session, through)?;
    Ok((
        Output::Rewound(Box::new(Rewind {
            session,
            revision: revision + 1,
            through,
            backup: backup.clone(),
        })),
        Some(Event::SessionRewound {
            session: Box::new(commands::session(db, session)?),
            backup: Box::new(backup),
        }),
    ))
}
