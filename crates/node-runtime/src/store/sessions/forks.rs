//! Forks share immutable turn history, but own their configuration and subsequent execution.
use super::*;
use crate::store::agent;
use sailry_protocol::conversation::{Fork, Status};

pub(in crate::store) fn fork(
    db: &Connection,
    source: SessionId,
    through: TurnId,
    expected_revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    create(db, source, through, expected_revision, false)
}

pub(super) fn backup(
    db: &Connection,
    source: SessionId,
    through: TurnId,
    expected_revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    create(db, source, through, expected_revision, true)
}

fn create(
    db: &Connection,
    source: SessionId,
    through: TurnId,
    expected_revision: u64,
    preserve_current: bool,
) -> Result<(Output, Option<Event>), Fault> {
    let previous = commands::session(db, source)?;
    writable(&previous)?;
    commands::check_revision(previous.revision, expected_revision)?;
    let boundary = agent::visible_run(db, source, through)?;
    if matches!(
        boundary.status,
        Status::Queued | Status::Running | Status::Stopping
    ) {
        return Err(Fault::new(ErrorCode::Busy, "fork target has not finished"));
    }
    let active: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation_turns h JOIN turns t ON t.id=h.turn
         JOIN agent_runs a ON a.turn=h.turn WHERE h.session=?1 AND t.rowid<=?2
         AND json_extract(a.body,'$.status') IN ('running','stopping'))",
            params![source.to_string(), boundary.sequence as i64],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if active {
        return Err(Fault::new(
            ErrorCode::Busy,
            "fork history contains an active turn",
        ));
    }
    // Inherit an already admitted configuration. Revoked credentials prevent execution,
    // but must not prevent preserving or branching read-only history.
    let mut session = Session {
        id: SessionId::new(),
        archived: false,
        revision: 1,
        fork: None,
        ..previous
    };
    insert(db, &session)?;
    db.execute(
        "INSERT INTO agent_state(owner,key,body) SELECT ?1,key,body FROM agent_state WHERE owner=?2 AND key=?3",
        params![session.id.to_string(), source.to_string(), agent::SESSION_TITLE],
    )
    .map_err(storage_error)?;
    crate::store::media::freeze(db, session.id, Some(source))?;
    // Membership is structural metadata. Native ADK events, usage and approvals are not copied.
    db.execute(
        "INSERT INTO conversation_turns(session,turn)
         SELECT ?1,h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn
         JOIN agent_runs a ON a.turn=h.turn WHERE h.session=?2 AND t.rowid<=?3
         AND json_extract(a.body,'$.status')!='queued'",
        params![
            session.id.to_string(),
            source.to_string(),
            boundary.sequence as i64
        ],
    )
    .map_err(storage_error)?;
    db.execute(
        "INSERT INTO session_forks(session,source,through_turn) VALUES(?1,?2,?3)",
        params![
            session.id.to_string(),
            source.to_string(),
            through.to_string()
        ],
    )
    .map_err(storage_error)?;
    agent::rebuild_event_index(db, session.id)?;
    agent::rebuild_session_state(db, session.id)?;
    crate::store::plugins::storage::conversation::fork(
        db,
        source,
        session.id,
        through,
        preserve_current,
    )?;
    session.activity = agent::activity::read(db, session.id)?;
    session.fork = Some(Fork {
        session: source,
        through,
    });
    Ok((
        Output::Session(session.clone()),
        Some(Event::SessionChanged(Box::new(session))),
    ))
}

pub(in crate::store) fn origin(db: &Connection, session: SessionId) -> Result<Option<Fork>, Fault> {
    let row: Option<(String, String)> = db
        .query_row(
            "SELECT source,through_turn FROM session_forks WHERE session=?1",
            [session.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    row.map(|(session, through)| {
        Ok(Fork {
            session: session.parse().map_err(storage_error)?,
            through: through.parse().map_err(storage_error)?,
        })
    })
    .transpose()
}
