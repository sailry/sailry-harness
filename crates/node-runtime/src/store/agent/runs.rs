use super::*;

pub(in crate::store) fn admit(
    db: &Connection,
    turn: &QueuedTurn,
    ready: bool,
) -> Result<(), Fault> {
    record(db, turn, Status::Queued, ready)?;
    super::queue::admit(db, turn, ready)
}

pub(super) fn record(
    db: &Connection,
    turn: &QueuedTurn,
    status: Status,
    ready: bool,
) -> Result<(), Fault> {
    let mut provider = match super::super::sessions::profile(db, turn.session, turn.revision)? {
        Some(profile) => Some(profile.provider),
        None => providers::get(db, turn.config.provider)?,
    };
    if let Some(provider) = &mut provider {
        crate::providers::login::capture(provider)?;
        // Freeze automatic tool availability at admission, never at execution/replay.
        crate::providers::search::enable(provider);
    }
    if let Some(provider) = &provider {
        providers::selection(provider, &turn.config)?;
    }
    let provider = provider.as_ref().map(encode).transpose()?;
    let worktree: String = db
        .query_row(
            "SELECT worktree FROM session_revisions WHERE session=?1 AND revision=?2",
            params![turn.session.to_string(), turn.revision as i64],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let run = Run {
        worktree: worktree.parse().map_err(storage_error)?,
        turn: turn.id,
        kind: turn.kind,
        session: turn.session,
        sequence: db
            .query_row(
                "SELECT rowid FROM turns WHERE id=?1",
                [turn.id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .map_err(storage_error)? as u64,
        revision: turn.revision,
        status,
        error: None,
        started_ms: (status == Status::Running).then(|| chrono::Utc::now().timestamp_millis()),
        finished_ms: None,
        origin: None,
    };
    db.execute(
        "INSERT INTO agent_runs(turn,ready,body,provider) VALUES(?1,?2,?3,?4)",
        params![turn.id.to_string(), ready, encode(&run)?, provider],
    )
    .map_err(storage_error)?;
    db.execute(
        "INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)",
        params![turn.session.to_string(), turn.id.to_string()],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn get(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_runs WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "turn does not exist"))?,
    )
    .map_err(storage_error)
}

pub(in crate::store) fn visible(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
) -> Result<Run, Fault> {
    let run = get(db, turn)?;
    let visible: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation_turns WHERE session=?1 AND turn=?2)",
            params![session.to_string(), turn.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !visible {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "turn is not in this conversation",
        ));
    }
    Ok(project(run, session))
}

pub(super) fn project(mut run: Run, session: SessionId) -> Run {
    if run.session != session {
        run.origin = Some(run.session);
        run.session = session;
    }
    run
}

fn put(db: &Connection, run: &Run) -> Result<(), Fault> {
    if run.kind == RunKind::Task
        && matches!(
            run.status,
            Status::Completed | Status::Failed | Status::Interrupted
        )
        && matches!(
            get(db, run.turn)?.status,
            Status::Queued | Status::Running | Status::Stopping
        )
    {
        super::super::sessions::attention::notify(db, run.session)?;
    }
    db.execute(
        "UPDATE agent_runs SET body=?2 WHERE turn=?1",
        params![run.turn.to_string(), encode(run)?],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn list(
    db: &Connection,
    session: SessionId,
    entries: &[Entry],
) -> Result<Vec<Run>, Fault> {
    let turns: Vec<_> = entries.iter().map(|entry| entry.turn).collect();
    let mut query = db.prepare("WITH recent AS (SELECT a.body,t.rowid AS position FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,t.rowid DESC LIMIT 100) SELECT body FROM (SELECT body,position FROM recent UNION SELECT a.body,t.rowid AS position FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND a.turn IN (SELECT value FROM json_each(?2))) ORDER BY position").map_err(storage_error)?;
    let runs = query
        .query_map(
            params![
                session.to_string(),
                serde_json::to_string(&turns).map_err(storage_error)?
            ],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(storage_error)?
        .map(|body| serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error))
        .collect::<Result<Vec<_>, Fault>>()?;
    Ok(runs)
}

pub(in crate::store) fn start(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let run = get(db, turn)?;
    if run.status != Status::Queued {
        return Err(Fault::new(ErrorCode::Conflict, "turn is no longer queued"));
    }
    db.execute(
        "UPDATE agent_runs SET ready=1 WHERE turn=?1",
        [turn.to_string()],
    )
    .map_err(storage_error)?;
    super::queue::bump(db, run.session)?;
    Ok(run)
}

pub(in crate::store) fn stop(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let mut run = get(db, turn)?;
    visible(db, run.session, turn)?;
    if run.status == Status::Queued {
        super::queue::remove(db, turn, run.session)?;
        super::super::attachments::turns::clear(db, turn)?;
    } else if run.status == Status::Running {
        super::queue::pause(db, run.session, true)?;
    }
    run.status = match run.status {
        Status::Queued => Status::Cancelled,
        Status::Running => Status::Stopping,
        status => status,
    };
    put(db, &run)?;
    Ok(run)
}

pub(in crate::store) fn stopping(db: &Connection, turn: TurnId) -> bool {
    get(db, turn).is_ok_and(|run| matches!(run.status, Status::Stopping | Status::Cancelled))
}

pub(super) fn claim(
    database: &mut Database,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Invocation>, Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let mut query = transaction.prepare("SELECT a.body,a.provider,t.caller,r.worktree FROM agent_runs a JOIN agent_pending p ON p.turn=a.turn JOIN turns t ON t.id=a.turn JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision JOIN agent_queues q ON q.session=t.session WHERE a.ready=1 AND q.paused=0 AND json_extract(a.body,'$.status')='queued' AND NOT EXISTS(SELECT 1 FROM agent_runs active JOIN turns other ON other.id=active.turn WHERE other.session=t.session AND json_extract(active.body,'$.status') IN ('running','stopping')) ORDER BY p.position,t.rowid LIMIT 1").map_err(storage_error)?;
    let row = query
        .query_row([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Option<Vec<u8>>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .optional()
        .map_err(storage_error)?;
    drop(query);
    let Some((body, provider, caller, worktree)) = row else {
        return Ok(None);
    };
    let mut run: Run = serde_json::from_slice(&body).map_err(storage_error)?;
    if let Some(fault) = super::continuation::check(&transaction, &run)? {
        run = stop(&transaction, run.turn)?;
        run.error = Some(fault);
        run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        put(&transaction, &run)?;
        let envelope = changed(&transaction, database.node, run.session)?;
        let queue = super::queue::read(&transaction, run.session)?;
        transaction.commit().map_err(storage_error)?;
        let _ = events.send(envelope);
        database
            .feeds
            .publish(database.node, run.session, Change::Queue(queue));
        database
            .feeds
            .publish(database.node, run.session, Change::Run(run));
        return claim(database, events);
    }
    let queued = super::queue::message(&transaction, run.turn)?;
    let plugins = if queued.turn.kind == RunKind::Task {
        crate::store::agent::plugins::packages(&transaction, &queued.turn.plugins)?
    } else {
        Vec::new()
    };
    let plugin_settings = super::super::plugins::settings::capture(&transaction, &plugins);
    let invocation = Invocation {
        automatic: transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_continuations WHERE turn=?1)",
                [queued.turn.id.to_string()],
                |row| row.get(0),
            )
            .map_err(storage_error)?,
        project: transaction
            .query_row(
                "SELECT project FROM worktrees WHERE id=?1",
                [&worktree],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(storage_error)?
            .map(|id| id.parse().map_err(storage_error))
            .transpose()?,
        connections: connections::catalog(&transaction, queued.turn.session)?,
        caller: NodeId(
            caller
                .try_into()
                .map_err(|_| storage_error("invalid caller identity"))?,
        ),
        worktree: worktree.parse().map_err(storage_error)?,
        plugins,
        plugin_settings,
        media: super::super::media::session_models(&transaction, queued.turn.session)?,
        turn: queued.turn,
        provider: provider
            .map(|body| serde_json::from_slice(&body).map_err(storage_error))
            .transpose()?,
        message: queued.message,
        child: None,
    };
    run.status = Status::Running;
    run.started_ms = Some(chrono::Utc::now().timestamp_millis());
    put(&transaction, &run)?;
    super::queue::remove(&transaction, run.turn, run.session)?;
    let queue = super::queue::read(&transaction, run.session)?;
    let envelope = changed(&transaction, database.node, run.session)?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    database
        .feeds
        .publish(database.node, run.session, Change::Queue(queue));
    database
        .feeds
        .publish(database.node, run.session, Change::Run(run));
    Ok(Some(invocation))
}

pub(super) fn finish(
    database: &mut Database,
    turn: TurnId,
    status: Status,
    error: Option<Fault>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let mut run = get(&transaction, turn)?;
    if !matches!(run.status, Status::Running | Status::Stopping) {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "turn execution has already ended",
        ));
    }
    run.status = if run.status == Status::Stopping {
        Status::Cancelled
    } else {
        status
    };
    run.error = error;
    run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
    put(&transaction, &run)?;
    let approvals = super::approvals::close(
        &transaction,
        turn,
        if run.status == Status::Interrupted {
            ApprovalState::Interrupted
        } else {
            ApprovalState::Cancelled
        },
    )?;
    if run.status == Status::Interrupted {
        super::queue::pause(&transaction, run.session, true)?;
    }
    let questions = super::questions::close(
        &transaction,
        turn,
        if run.status == Status::Interrupted {
            question::State::Interrupted
        } else {
            question::State::Cancelled
        },
    )?;
    let queue = super::queue::read(&transaction, run.session)?;
    let envelope = changed(&transaction, database.node, run.session)?;
    let child = super::children::get(&transaction, &run)?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    database.approvals.retain(|_, (owner, _)| *owner != turn);
    database.questions.retain(|_, (owner, _)| *owner != turn);
    for question in questions {
        database
            .feeds
            .publish(database.node, run.session, Change::Question(question));
    }
    for approval in approvals {
        database
            .feeds
            .publish(database.node, run.session, Change::Approval(approval));
    }
    database
        .feeds
        .publish(database.node, run.session, Change::Queue(queue));
    database
        .feeds
        .publish(database.node, run.session, Change::Run(run));
    if let Some(child) = child {
        database
            .feeds
            .publish(database.node, child.origin.session, Change::Child(child));
    }
    Ok(())
}

pub(in crate::store) fn recover(db: &Connection) -> Result<(), Fault> {
    super::continuation::recover(db)?;
    let mut query = db.prepare("SELECT body FROM agent_runs WHERE json_extract(body,'$.status') IN ('running','stopping')").map_err(storage_error)?;
    let rows = query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?;
    for body in rows {
        let mut run: Run =
            serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
        run.status = Status::Interrupted;
        super::approvals::close(db, run.turn, ApprovalState::Interrupted)?;
        super::questions::close(db, run.turn, question::State::Interrupted)?;
        super::queue::pause(db, run.session, true)?;
        run.error = Some(Fault::new(
            ErrorCode::OutcomeUnknown,
            "execution was interrupted; model and tool calls were not replayed",
        ));
        put(db, &run)?;
    }
    Ok(())
}
