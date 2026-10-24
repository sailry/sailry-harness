//! Rewinds change visible membership, never canonical history or external side effects.
use super::*;
use crate::store::agent;
use sailry_protocol::conversation::Rewind;

pub(in crate::store) async fn revision(db: &Connection, session: SessionId) -> Result<u64, Fault> {
    match db
        .prepare("SELECT history_revision FROM sessions WHERE id=?1")
        .await
        .map_err(storage_error)?
        .query_row([session.to_string()])
        .await
        .and_then(|row| row.get::<i64>(0).map(|revision| revision as u64))
    {
        Ok(value) => Ok(Some(value)),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    }
    .map_err(storage_error)?
    .ok_or_else(|| Fault::new(ErrorCode::NotFound, "session does not exist"))
}

pub(in crate::store) async fn rewind(
    db: &Connection,
    session: SessionId,
    through: Option<TurnId>,
    expected_head: TurnId,
    expected_revision: u64,
    backup: bool,
) -> Result<(Output, Option<Event>), Fault> {
    let current = commands::session(db, session).await?;
    writable(&current)?;
    let revision = revision(db, session).await?;
    commands::check_revision(revision, expected_revision)?;
    let busy: bool = db.prepare("SELECT EXISTS(SELECT 1 FROM conversation_turns h JOIN agent_runs a ON a.turn=h.turn
             WHERE h.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))").await.map_err(storage_error)?.query_row([session.to_string()]).await.and_then(|row| row.get(0))
        .map_err(storage_error)?;
    if busy {
        return Err(Fault::new(
            ErrorCode::Busy,
            "conversation has active or queued turns",
        ));
    }
    let head: Option<String> = match db
        .prepare(
            "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn
             WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1",
        )
        .await
        .map_err(storage_error)?
        .query_row([session.to_string()])
        .await
        .and_then(|row| row.get(0))
    {
        Ok(value) => Ok(Some(value)),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    }
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
    let boundary = if let Some(turn) = through {
        Some(agent::visible_run(db, session, turn).await?)
    } else {
        None
    };
    let backup = if backup {
        let (Output::Session(backup), _) =
            super::forks::backup(db, session, expected_head, current.revision).await?
        else {
            unreachable!("history backup returns a session")
        };
        Some(backup)
    } else {
        None
    };
    db.execute(
        "DELETE FROM conversation_turns WHERE session=?1 AND turn IN
         (SELECT id FROM turns WHERE rowid>?2)",
        params![
            session.to_string(),
            boundary.map_or(0, |run| run.sequence) as i64
        ],
    )
    .await
    .map_err(storage_error)?;
    db.execute(
        "UPDATE sessions SET history_revision=history_revision+1 WHERE id=?1",
        [session.to_string()],
    )
    .await
    .map_err(storage_error)?;
    agent::rebuild_event_index(db, session).await?;
    agent::rebuild_session_state(db, session).await?;
    crate::store::plugins::storage::conversation::rewind(db, session, through).await?;
    Ok((
        Output::Rewound(Box::new(Rewind {
            session,
            revision: revision + 1,
            through,
            backup: backup.clone(),
        })),
        Some(Event::SessionRewound {
            session: Box::new(commands::session(db, session).await?),
            backup: backup.map(Box::new),
        }),
    ))
}
