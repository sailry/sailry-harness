//! Durable guidance hands off only at ADK's settled before-model boundary.
use super::*;

#[cfg(test)]
mod tests;

pub(in crate::store) fn recipient(
    db: &Connection,
    turn: TurnId,
    expected_revision: Option<u64>,
) -> Result<Run, Fault> {
    let run = runs::get(db, turn)?;
    if run.kind != RunKind::Task || run.status != Status::Running {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance requires a running task recipient",
        ));
    }
    if let Some(revision) = expected_revision {
        check_revision(run.revision, revision)?;
    }
    Ok(run)
}

pub(in crate::store) fn pending(
    db: &Connection,
    recipient: TurnId,
) -> Result<Option<TurnId>, Fault> {
    db.query_row(
        "SELECT p.turn FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn WHERE p.recipient=?1 AND json_extract(a.body,'$.status')='queued' ORDER BY p.position LIMIT 1",
        [recipient.to_string()],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(storage_error)?
    .map(|turn| turn.parse().map_err(storage_error))
    .transpose()
}

fn compatible(db: &Connection, source: &Run, target: &Run) -> Result<(), Fault> {
    let provider = |turn: TurnId| -> Result<Option<Vec<u8>>, Fault> {
        db.query_row(
            "SELECT provider FROM agent_runs WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)
    };
    if source.kind != RunKind::Task
        || source.session != target.session
        || source.worktree != target.worktree
        || super::super::permissions::config(db, source.turn)?
            != super::super::permissions::config(db, target.turn)?
        || super::super::super::sessions::roles::read(db, source.session, source.revision)?
            != super::super::super::sessions::roles::read(db, target.session, target.revision)?
        || super::super::plugins::read(db, source.turn)?
            != super::super::plugins::read(db, target.turn)?
        || provider(source.turn)? != provider(target.turn)?
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance must retain the recipient's frozen configuration and capabilities",
        ));
    }
    Ok(())
}

pub(in crate::store) fn tag(db: &Connection, source: TurnId, target: TurnId) -> Result<(), Fault> {
    let source = runs::get(db, source)?;
    if source.status != Status::Queued {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance input is not queued",
        ));
    }
    let target = recipient(db, target, None)?;
    compatible(db, &source, &target)?;
    db.execute(
        "UPDATE agent_pending SET recipient=?2,revision=revision+1 WHERE turn=?1",
        params![source.turn.to_string(), target.turn.to_string()],
    )
    .map_err(storage_error)?;
    runs::start(db, source.turn)?;
    let first = read(db, source.session)?
        .items
        .first()
        .map(|item| item.turn);
    reorder(db, source.session, source.turn, first)?;
    pause(db, source.session, false)
}

pub(super) fn select(
    db: &Connection,
    source: TurnId,
    target: TurnId,
    expected_revision: u64,
) -> Result<SessionId, Fault> {
    let source = check_item(db, source, expected_revision)?;
    let session = super::super::super::commands::session(db, source.session)?;
    super::super::super::sessions::writable(&session)?;
    let target = recipient(db, target, None)?;
    compatible(db, &source, &target)?;
    // Retargeting never discards the previous pending input.
    clear(db, source.session, None)?;
    tag(db, source.turn, target.turn)?;
    Ok(source.session)
}

pub(in crate::store) fn clear(
    db: &Connection,
    session: SessionId,
    recipient: Option<TurnId>,
) -> Result<(), Fault> {
    if db
        .execute(
            "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE recipient IS NOT NULL AND (?2 IS NULL OR recipient=?2) AND turn IN (SELECT id FROM turns WHERE session=?1)",
            params![session.to_string(), recipient.map(|turn| turn.to_string())],
        )
        .map_err(storage_error)?
        > 0
    {
        bump(db, session)?;
    }
    Ok(())
}

pub(in crate::store) fn recover(db: &Connection) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT DISTINCT t.session FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE p.recipient IS NOT NULL")
        .map_err(storage_error)?;
    let sessions = query
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(query);
    for session in sessions {
        let session = session.parse().map_err(storage_error)?;
        pause(db, session, true)?;
        clear(db, session, None)?;
    }
    Ok(())
}

pub(in crate::store) fn yield_run(
    database: &mut Database,
    target: TurnId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<bool, Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let target = runs::get(&transaction, target)?;
    if target.kind != RunKind::Task || target.status != Status::Running {
        return Ok(false);
    }
    // A later explicit Stop wins over admission; never revive a paused intent.
    if read(&transaction, target.session)?.paused {
        return Ok(false);
    }
    let Some(source) = pending(&transaction, target.turn)? else {
        return Ok(false);
    };
    compatible(&transaction, &runs::get(&transaction, source)?, &target)?;
    let run = runs::handoff(&transaction, target.turn)?;
    let queue = read(&transaction, target.session)?;
    let envelope = super::super::changed(&transaction, database.node, target.session)?;
    let child = super::super::children::get(&transaction, &run)?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    database
        .feeds
        .publish(database.node, target.session, Change::Queue(queue));
    database
        .feeds
        .publish(database.node, target.session, Change::Run(run));
    if let Some(child) = child {
        database
            .feeds
            .publish(database.node, child.origin.session, Change::Child(child));
    }
    Ok(true)
}

pub(in crate::store) fn retain(
    database: &mut Database,
    target: TurnId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let run = runs::get(&transaction, target)?;
    let child: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=?1)",
        [run.session.to_string()],
        |row| row.get(0),
    ).map_err(storage_error)?;
    if child {
        super::super::delegation::retain_guidance(&transaction, Some(target))?;
    } else {
        pause(&transaction, run.session, true)?;
        clear(&transaction, run.session, Some(target))?;
    }
    let queue = read(&transaction, run.session)?;
    let envelope = super::super::changed(&transaction, database.node, run.session)?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    database
        .feeds
        .publish(database.node, run.session, Change::Queue(queue));
    Ok(())
}
