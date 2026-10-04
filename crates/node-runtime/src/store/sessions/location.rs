//! Execution location changes preserve the revision attached to every admitted turn.
use super::*;

fn target(
    db: &Connection,
    id: SessionId,
    worktree: WorktreeId,
    revision: u64,
) -> Result<Session, Fault> {
    let session = commands::session(db, id)?;
    writable(&session)?;
    commands::check_revision(session.revision, revision)?;
    super::super::worktrees::select(
        db,
        session.project.ok_or_else(|| {
            commands::invalid("connection conversations cannot move to a project")
        })?,
        Some(worktree),
    )?;
    let busy: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM conversation_turns h JOIN agent_runs a ON a.turn=h.turn
         WHERE h.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))",
        [id.to_string()], |row| row.get(0),
    ).map_err(storage_error)?;
    if busy {
        return Err(Fault::new(
            ErrorCode::Busy,
            "conversation has active or queued turns",
        ));
    }
    let path: String = db
        .query_row(
            "SELECT path FROM worktrees WHERE id=?1",
            [worktree.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !std::path::Path::new(&path).is_dir() {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "worktree directory is unavailable",
        ));
    }
    Ok(session)
}

pub(in crate::store) fn move_to(
    db: &Connection,
    id: SessionId,
    worktree: WorktreeId,
    revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = target(db, id, worktree, revision)?;
    if session.worktree == worktree {
        return Ok((Output::Session(session), None));
    }
    session.worktree = worktree;
    commands::revise_session(db, session)
}

pub(in crate::store) fn fork_at(
    db: &Connection,
    id: SessionId,
    worktree: WorktreeId,
    revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = target(db, id, worktree, revision)?;
    let head: Option<String> = db
        .query_row(
            "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn
         WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(head) = head {
        let (Output::Session(fork), _) =
            super::fork(db, id, head.parse().map_err(storage_error)?, revision)?
        else {
            unreachable!("fork returns a session")
        };
        session = fork;
    } else {
        session.id = SessionId::new();
        session.archived = false;
        session.revision = 1;
        session.fork = None;
        session.worktree = worktree;
        insert(db, &session)?;
        db.execute(
            "INSERT INTO agent_state(owner,key,body) SELECT ?1,key,body FROM agent_state WHERE owner=?2 AND key=?3",
            params![session.id.to_string(), id.to_string(), crate::store::agent::SESSION_TITLE],
        )
        .map_err(storage_error)?;
        crate::store::media::freeze(db, session.id, Some(id))?;
        return Ok((
            Output::Session(session.clone()),
            Some(Event::SessionChanged(Box::new(session))),
        ));
    }
    session.worktree = worktree;
    commands::revise_session(db, session)
}
