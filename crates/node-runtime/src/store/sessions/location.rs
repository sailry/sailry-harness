//! Execution location changes preserve the revision attached to every admitted turn.
use super::*;

async fn target(
    db: &Connection,
    id: SessionId,
    worktree: WorktreeId,
    revision: u64,
) -> Result<Session, Fault> {
    let session = commands::session(db, id).await?;
    writable(&session)?;
    commands::check_revision(session.revision, revision)?;
    super::super::worktrees::select(
        db,
        session.project.ok_or_else(|| {
            sailry_protocol::Fault::invalid("connection conversations cannot move to a project")
        })?,
        Some(worktree),
    )
    .await?;
    let busy: bool = db.prepare("SELECT EXISTS(SELECT 1 FROM conversation_turns h JOIN agent_runs a ON a.turn=h.turn
         WHERE h.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))").await.map_err(storage_error)?.query_row([id.to_string()]).await.and_then(|row| row.get(0)).map_err(storage_error)?;
    if busy {
        return Err(Fault::new(
            ErrorCode::Busy,
            "conversation has active or queued turns",
        ));
    }
    let path: String = db
        .prepare("SELECT path FROM worktrees WHERE id=?1")
        .await
        .map_err(storage_error)?
        .query_row([worktree.to_string()])
        .await
        .and_then(|row| row.get(0))
        .map_err(storage_error)?;
    if !std::path::Path::new(&path).is_dir() {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "worktree directory is unavailable",
        ));
    }
    Ok(session)
}

pub(in crate::store) async fn move_to(
    db: &Connection,
    id: SessionId,
    worktree: WorktreeId,
    revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = target(db, id, worktree, revision).await?;
    if session.worktree == worktree {
        return Ok((Output::Session(session), None));
    }
    session.worktree = worktree;
    commands::revise_session(db, session).await
}

pub(in crate::store) async fn fork_at(
    db: &Connection,
    id: SessionId,
    worktree: WorktreeId,
    revision: u64,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = target(db, id, worktree, revision).await?;
    let head: Option<String> = match db
        .prepare(
            "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn
         WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1",
        )
        .await
        .map_err(storage_error)?
        .query_row([id.to_string()])
        .await
        .and_then(|row| row.get(0))
    {
        Ok(value) => Ok(Some(value)),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    }
    .map_err(storage_error)?;
    if let Some(head) = head {
        let (Output::Session(fork), _) =
            super::fork(db, id, head.parse().map_err(storage_error)?, revision).await?
        else {
            unreachable!("fork returns a session")
        };
        session = fork;
    } else {
        session.id = SessionId::new();
        session.creation_order = 0;
        session.archived = false;
        session.revision = 1;
        session.fork = None;
        session.worktree = worktree;
        session.creation_order = insert(db, &session).await?;
        db.execute(
            "INSERT INTO agent_state(owner,key,body) SELECT ?1,key,body FROM agent_state WHERE owner=?2 AND key=?3",
            params![session.id.to_string(), id.to_string(), crate::store::agent::SESSION_TITLE],
        ).await
        .map_err(storage_error)?;
        crate::store::media::freeze(db, session.id, Some(id)).await?;
        return Ok((
            Output::Session(session.clone()),
            Some(Event::SessionChanged(Box::new(session))),
        ));
    }
    session.worktree = worktree;
    commands::revise_session(db, session).await
}
