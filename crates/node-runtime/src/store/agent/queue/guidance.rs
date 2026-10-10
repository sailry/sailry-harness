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

fn same_capture(db: &Connection, source: &Run, target: &Run) -> Result<bool, Fault> {
    let provider = |turn: TurnId| -> Result<Option<Vec<u8>>, Fault> {
        db.query_row(
            "SELECT provider FROM agent_runs WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)
    };
    Ok(source.kind == RunKind::Task
        && source.session == target.session
        && source.worktree == target.worktree
        && super::super::permissions::config(db, source.turn)?
            == super::super::permissions::config(db, target.turn)?
        && super::super::super::sessions::roles::read(db, source.session, source.revision)?
            == super::super::super::sessions::roles::read(db, target.session, target.revision)?
        && super::super::plugins::read(db, source.turn)?
            == super::super::plugins::read(db, target.turn)?
        && super::super::super::hooks::frozen(db, source.turn)?
            == super::super::super::hooks::frozen(db, target.turn)?
        && provider(source.turn)? == provider(target.turn)?)
}

fn compatible(db: &Connection, source: &Run, target: &Run) -> Result<(), Fault> {
    if !same_capture(db, source, target)? {
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
    caller: NodeId,
    request: &Request,
    source: TurnId,
    target: TurnId,
    expected_revision: u64,
) -> Result<(SessionId, Vec<Run>), Fault> {
    let source = check_item(db, source, expected_revision)?;
    let session = super::super::super::commands::session(db, source.session)?;
    super::super::super::sessions::writable(&session)?;
    let target = recipient(db, target, None)?;
    if source.kind != RunKind::Task
        || source.session != target.session
        || source.worktree != target.worktree
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance belongs to another execution scope",
        ));
    }
    // Retargeting never discards the previous pending input.
    clear(db, source.session, None)?;
    let mut changed = Vec::new();
    let selected = if same_capture(db, &source, &target)? {
        source.turn
    } else {
        // This explicit guidance command admits a new turn with the recipient's
        // capture. The original sent turn and its configuration remain immutable.
        let queued = message(db, source.turn)?;
        let input: (String, Vec<u8>) = db
            .query_row(
                "SELECT request,caller FROM agent_pending WHERE turn=?1",
                [source.turn.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(storage_error)?;
        let turn = inherit(db, caller, request, &target, &queued.message)?;
        changed.push(runs::stop(db, source.turn)?);
        // Keep the exact current message ledger reference, including prior edits.
        db.execute(
            "UPDATE agent_pending SET request=?2,caller=?3 WHERE turn=?1",
            params![turn.id.to_string(), input.0, input.1],
        )
        .map_err(storage_error)?;
        changed.push(runs::get(db, turn.id)?);
        turn.id
    };
    tag(db, selected, target.turn)?;
    Ok((source.session, changed))
}

/// Admits input with the recipient's frozen configuration and capabilities.
pub(in crate::store) fn inherit(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    target: &Run,
    message: &Input,
) -> Result<QueuedTurn, Fault> {
    super::validate(message)?;
    let mut session = super::super::super::commands::session(db, target.session)?;
    session.config = super::super::permissions::config(db, target.turn)?;
    session.roles =
        super::super::super::sessions::roles::read(db, target.session, target.revision)?;
    let plugins = super::super::plugins::read(db, target.turn)?;
    super::super::references::validate(db, &session, message, Some(&plugins))?;
    let turn = QueuedTurn {
        id: TurnId::new(),
        kind: RunKind::Task,
        session: target.session,
        request: request.id,
        revision: target.revision,
        config: session.config,
        roles: session.roles,
        plugins,
    };
    db.execute(
        "INSERT INTO turns(id,kind,session,revision,request,caller,plugins) VALUES(?1,'task',?2,?3,?4,?5,?6)",
        params![turn.id.to_string(), turn.session.to_string(), turn.revision as i64,
            request.id.to_string(), &caller.0[..], encode(&turn.plugins)?],
    ).map_err(storage_error)?;
    super::super::super::attachments::turns::bind(db, turn.id, &message.attachments)?;
    runs::inherit(db, &turn, target.turn, Status::Queued, true)?;
    admit(db, &turn, true)?;
    Ok(turn)
}

/// Main-session guidance admits and selects input in one controller command.
pub(in crate::store) fn guide(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    target: Run,
) -> Result<(Output, Option<Event>), Fault> {
    let Command::GuideTurn { message, .. } = &request.command else {
        unreachable!()
    };
    if request.plugin.is_some() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "guidance requires a delegated child",
        ));
    }
    super::super::super::sessions::writable(&super::super::super::commands::session(
        db,
        target.session,
    )?)?;
    let earlier = pending(db, target.turn)?;
    let turn = inherit(db, caller, request, &target, message)?;
    if let Some(earlier) = earlier {
        // Later guidance waits directly behind the input already selected for this boundary.
        let next = read(db, target.session)?
            .items
            .iter()
            .skip_while(|item| item.turn != earlier)
            .nth(1)
            .map(|item| item.turn)
            .filter(|next| *next != turn.id);
        if next.is_some() {
            reorder(db, target.session, turn.id, next)?;
        }
    } else {
        tag(db, turn.id, target.turn)?;
    }
    Ok((
        Output::QueuedTurn(turn.clone()),
        Some(super::super::activity::queued(db, turn)?),
    ))
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
