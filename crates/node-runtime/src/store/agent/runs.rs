use super::*;

mod input;
pub(super) use input::{capture, hydrate};

pub(in crate::store) fn admit(
    db: &Connection,
    turn: &QueuedTurn,
    ready: bool,
) -> Result<(), Fault> {
    record_with(db, turn, Status::Queued, ready, true)?;
    super::queue::admit(db, turn, ready)
}

pub(super) fn record(
    db: &Connection,
    turn: &QueuedTurn,
    status: Status,
    ready: bool,
) -> Result<(), Fault> {
    record_with(db, turn, status, ready, false)
}

fn record_with(
    db: &Connection,
    turn: &QueuedTurn,
    status: Status,
    ready: bool,
    fresh: bool,
) -> Result<(), Fault> {
    let mut provider = match super::super::sessions::profile(db, turn.session, turn.revision)? {
        Some(profile) => Some(profile.provider),
        None => providers::get(db, turn.config.provider)?,
    };
    if let Some(provider) = &mut provider {
        if fresh {
            crate::store::providers::presets::refresh(db, provider)?;
        } else {
            crate::store::providers::presets::capture(db, provider)?;
        }
        crate::providers::login::capture(provider)?;
        // Freeze automatic tool availability at admission, never at execution/replay.
        super::super::providers::completion::fill(db, provider)?;
    }
    if let Some(provider) = &provider {
        providers::selection(provider, &turn.config)?;
    }
    let provider = provider.as_ref().map(encode).transpose()?;
    insert(db, turn, status, ready, provider, None)
}

pub(super) fn inherit(
    db: &Connection,
    turn: &QueuedTurn,
    source: TurnId,
    status: Status,
    ready: bool,
) -> Result<(), Fault> {
    let provider: Option<Vec<u8>> = db
        .query_row(
            "SELECT provider FROM agent_runs WHERE turn=?1",
            [source.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    insert(
        db,
        turn,
        status,
        ready,
        provider,
        Some(super::super::hooks::frozen(db, source)?),
    )
}

fn insert(
    db: &Connection,
    turn: &QueuedTurn,
    status: Status,
    ready: bool,
    provider: Option<Vec<u8>>,
    hooks: Option<sailry_protocol::hooks::Settings>,
) -> Result<(), Fault> {
    let worktree: String = db
        .query_row(
            "SELECT worktree FROM session_revisions WHERE session=?1 AND revision=?2",
            params![turn.session.to_string(), turn.revision as i64],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let hooks = hooks.map(Ok).unwrap_or_else(|| {
        super::super::hooks::read(db, worktree.parse().map_err(storage_error)?)
    })?;
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
        continuation: None,
        notice: false,
        notices: Vec::new(),
        input: None,
    };
    db.execute(
        "INSERT INTO agent_runs(turn,ready,body,provider,hooks) VALUES(?1,?2,?3,?4,?5)",
        params![
            turn.id.to_string(),
            ready,
            encode(&run)?,
            provider,
            encode(&hooks)?
        ],
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

pub(super) fn put(db: &Connection, run: &Run) -> Result<(), Fault> {
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
    let mut stored = run.clone();
    stored.input = None;
    db.execute(
        "UPDATE agent_runs SET body=?2 WHERE turn=?1",
        params![run.turn.to_string(), encode(&stored)?],
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
        super::background::release(db, turn, true)?;
        super::queue::remove(db, turn, run.session)?;
        super::super::attachments::turns::clear(db, turn)?;
    } else if matches!(run.status, Status::Running | Status::Stopping) {
        super::queue::pause(db, run.session, true)?;
        super::queue::guidance::clear(db, run.session, Some(turn))?;
    }
    run.status = match run.status {
        Status::Queued => Status::Cancelled,
        Status::Running => Status::Stopping,
        status => status,
    };
    put(db, &run)?;
    Ok(run)
}

pub(super) fn handoff(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let mut run = get(db, turn)?;
    if run.status != Status::Running {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance recipient is not running",
        ));
    }
    run.status = Status::Stopping;
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
    claim_with(database, None, events)
}

pub(super) fn claim_guidance(
    database: &mut Database,
    recipient: TurnId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Invocation>, Fault> {
    let run = get(&database.connection, recipient)?;
    if matches!(run.status, Status::Running | Status::Stopping) {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance recipient has not ended",
        ));
    }
    if super::queue::read(&database.connection, run.session)?.paused {
        super::queue::guidance::retain(database, recipient, events)?;
        return Ok(None);
    }
    claim_with(database, Some(recipient), events)
}

fn claim_with(
    database: &mut Database,
    recipient: Option<TurnId>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Invocation>, Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let mut query = transaction.prepare("SELECT a.body,a.provider,t.caller,r.worktree FROM agent_runs a JOIN agent_pending p ON p.turn=a.turn JOIN turns t ON t.id=a.turn JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision JOIN agent_queues q ON q.session=t.session WHERE a.ready=1 AND q.paused=0 AND json_extract(a.body,'$.status')='queued' AND ((?1 IS NULL AND NOT EXISTS(SELECT 1 FROM agent_delegations d JOIN turns child ON child.id=d.turn WHERE child.session=t.session)) OR (?1 IS NOT NULL AND p.recipient=?1)) AND NOT EXISTS(SELECT 1 FROM agent_runs active JOIN turns other ON other.id=active.turn WHERE other.session=t.session AND json_extract(active.body,'$.status') IN ('running','stopping')) ORDER BY p.position,t.rowid LIMIT 1").map_err(storage_error)?;
    let row = query
        .query_row([recipient.map(|turn| turn.to_string())], |row| {
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
    if let Some(fault) = super::continuation::check(&transaction, &run)?
        .or(super::background::check(&transaction, &run)?)
    {
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
        super::feed::run(database, run);
        return claim_with(database, recipient, events);
    }
    let queued = super::queue::message(&transaction, run.turn)?;
    let plugins = if queued.turn.kind == RunKind::Task {
        crate::store::agent::plugins::packages(&transaction, &queued.turn.plugins)?
    } else {
        Vec::new()
    };
    let plugin_settings = super::super::plugins::settings::capture(&transaction, &plugins);
    let provider: Option<Provider> = provider
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?;
    if let Some(provider) = &provider {
        crate::store::providers::presets::verify(&transaction, provider)?;
    }
    let media = super::super::media::for_turn(
        super::super::media::session_models(&transaction, queued.turn.session)?,
        provider.as_ref(),
        &queued.turn.config,
    );
    let background = super::background::bind(&transaction, &mut run)?;
    let invocation = Invocation {
        instructions: super::super::captures::instructions(&transaction, run.turn)?,
        notice_input: super::background::render(&transaction, &run.notices)?,
        notices: run.notices.clone(),
        notice: run.notice,
        hooks: super::super::hooks::frozen(&transaction, queued.turn.id)?,
        title: super::activity::assigned_title(&transaction, queued.turn.session)?,
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
        media,
        turn: queued.turn,
        provider,
        message: queued.message,
        child: if recipient.is_some() {
            super::delegation::child(&transaction, run.turn)?
        } else {
            None
        },
    };
    let review_changes = if invocation.child.is_none()
        && !invocation.automatic
        && !run.notice
        && run.kind == RunKind::Task
    {
        super::review::supersede(&transaction, run.session)?
    } else {
        Vec::new()
    };
    if invocation.child.is_none() {
        run.status = Status::Running;
        run.started_ms = Some(chrono::Utc::now().timestamp_millis());
    }
    capture(
        &transaction,
        &invocation.turn,
        invocation.automatic || run.notice,
        &invocation.message,
    )?;
    put(&transaction, &run)?;
    super::queue::remove(&transaction, run.turn, run.session)?;
    let queue = super::queue::read(&transaction, run.session)?;
    let envelope = changed(&transaction, database.node, run.session)?;
    let child = super::children::get(&transaction, &run)?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    for (session, change) in review_changes {
        database.feeds.publish(database.node, session, change);
    }
    database
        .feeds
        .publish(database.node, run.session, Change::Queue(queue));
    let mut diagnostic =
        crate::logging::Entry::new(crate::logging::Area::Run, crate::logging::Phase::Begin);
    diagnostic.context = crate::logging::Context {
        node: Some(database.node),
        session: Some(run.session),
        turn: Some(run.turn),
        worktree: Some(run.worktree),
        ..Default::default()
    };
    diagnostic.status = Some(run.status);
    diagnostic.emit();
    super::feed::run(database, run);
    super::background::publish(database, background);
    if let Some(child) = child {
        database
            .feeds
            .publish(database.node, child.origin.session, Change::Child(child));
    }
    Ok(Some(invocation))
}

#[cfg(test)]
pub(super) fn finish(
    database: &mut Database,
    turn: TurnId,
    status: Status,
    error: Option<Fault>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Status, Fault> {
    finish_with(database, turn, status, error, None, events)
}

pub(super) fn finish_with(
    database: &mut Database,
    turn: TurnId,
    status: Status,
    error: Option<Fault>,
    review: Option<AdkEvent>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Status, Fault> {
    let mut run = get(&database.connection, turn)?;
    let queued_child = run.started_ms.is_none()
        && run.finished_ms.is_none()
        && matches!(run.status, Status::Queued | Status::Cancelled)
        && matches!(
            status,
            Status::Cancelled | Status::Interrupted | Status::Failed
        )
        && super::delegation::origin(&database.connection, turn)?.is_some();
    if !matches!(run.status, Status::Running | Status::Stopping) && !queued_child {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "turn execution has already ended",
        ));
    }
    run.status = if matches!(run.status, Status::Stopping | Status::Cancelled)
        && !matches!(status, Status::Continued | Status::Interrupted)
    {
        Status::Cancelled
    } else {
        status
    };
    if run.status != Status::Completed {
        super::history::retain(database, &run, events)?;
    }
    let transaction = database.connection.transaction().map_err(storage_error)?;
    run.error = error;
    run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
    if queued_child {
        super::queue::remove(&transaction, turn, run.session)?;
    }
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
    let review_changes = if run.status == Status::Completed {
        review
            .map(|event| super::review::publish(&transaction, &run, event))
            .transpose()?
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let queue = super::queue::read(&transaction, run.session)?;
    let envelope = changed(&transaction, database.node, run.session)?;
    let child = super::children::get(&transaction, &run)?;
    let status = run.status;
    let background = super::background::child_finished(&transaction, &run)?;
    let released = super::background::release(&transaction, turn, run.notice)?;
    let mut owners: std::collections::BTreeSet<_> =
        background.iter().map(|task| task.session).collect();
    owners.insert(run.session);
    let mut scheduled = Vec::new();
    for owner in owners {
        if let Some(turn) = super::background::schedule(&transaction, database.node, owner)? {
            scheduled.push(turn);
        }
    }
    transaction.commit().map_err(storage_error)?;
    let mut diagnostic = crate::logging::Entry::new(
        crate::logging::Area::Run,
        match status {
            Status::Completed => crate::logging::Phase::Completed,
            Status::Continued => crate::logging::Phase::Continued,
            Status::Cancelled => crate::logging::Phase::Cancelled,
            Status::Failed => crate::logging::Phase::Failed,
            _ => crate::logging::Phase::Interrupted,
        },
    );
    diagnostic.context = crate::logging::Context {
        node: Some(database.node),
        session: Some(run.session),
        turn: Some(run.turn),
        worktree: Some(run.worktree),
        ..Default::default()
    };
    diagnostic.status = Some(status);
    diagnostic.code = run.error.as_ref().map(|fault| fault.code);
    diagnostic.duration_us = run
        .started_ms
        .zip(run.finished_ms)
        .map(|(start, end)| end.saturating_sub(start).max(0) as u64 * 1000);
    diagnostic.emit();
    let _ = events.send(envelope);
    database
        .approvals
        .retain(|_, (owner, _, _, _)| *owner != turn);
    database.questions.retain(|_, (owner, _)| *owner != turn);
    // Publish completion consent before the terminal run so observers cannot
    // conclude the turn without its pending review from the same transaction.
    for (session, change) in review_changes {
        database.feeds.publish(database.node, session, change);
    }
    // The question is pending consent, never a Running or Waiting execution.
    super::feed::run(database, run.clone());
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
    if let Some(child) = child {
        database
            .feeds
            .publish(database.node, child.origin.session, Change::Child(child));
    }
    super::background::publish(database, background.into_iter().chain(released));
    for turn in scheduled {
        super::feed::run(database, get(&database.connection, turn.id)?);
        let queue = super::queue::read(&database.connection, turn.session)?;
        database
            .feeds
            .publish(database.node, turn.session, Change::Queue(queue));
        super::background::publish(
            database,
            super::background::list(&database.connection, turn.session)?,
        );
    }
    Ok(status)
}

pub(in crate::store) fn recover(db: &Connection) -> Result<(), Fault> {
    super::continuation::recover(db)?;
    super::queue::guidance::recover(db)?;
    super::delegation::retain_guidance(db, None)?;
    let mut query = db.prepare("SELECT a.body FROM agent_runs a WHERE json_extract(a.body,'$.status') IN ('running','stopping') OR (json_extract(a.body,'$.status')='queued' AND EXISTS(SELECT 1 FROM agent_delegations d WHERE d.turn=a.turn) AND NOT EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=a.turn))").map_err(storage_error)?;
    let rows = query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?;
    for body in rows {
        let mut run: Run =
            serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
        if run.status == Status::Queued {
            run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        }
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
    super::background::recover(db)?;
    super::queue::recover(db)
}
