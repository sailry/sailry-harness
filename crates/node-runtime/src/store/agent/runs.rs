use super::*;

pub(super) mod guidance;
mod input;
pub(super) use input::{capture, hydrate};

pub(in crate::store) async fn admit(
    db: &Connection,
    turn: &QueuedTurn,
    ready: bool,
) -> Result<(), Fault> {
    record_with(db, turn, Status::Queued, ready, true).await?;
    super::queue::admit(db, turn, ready).await
}

pub(super) async fn record(
    db: &Connection,
    turn: &QueuedTurn,
    status: Status,
    ready: bool,
) -> Result<(), Fault> {
    record_with(db, turn, status, ready, false).await
}

async fn record_with(
    db: &Connection,
    turn: &QueuedTurn,
    status: Status,
    ready: bool,
    fresh: bool,
) -> Result<(), Fault> {
    let mut provider =
        match super::super::sessions::profile(db, turn.session, turn.revision).await? {
            Some(profile) => Some(profile.provider),
            None => providers::get(db, turn.config.provider).await?,
        };
    if let Some(provider) = &mut provider {
        if fresh {
            crate::store::providers::presets::refresh(db, provider).await?;
        } else {
            crate::store::providers::presets::capture(db, provider).await?;
        }
        crate::providers::login::capture(provider)?;
        // Freeze automatic tool availability at admission, never at execution/replay.
        super::super::providers::completion::fill(db, provider).await?;
    }
    if let Some(provider) = &provider {
        providers::selection(provider, &turn.config)?;
    }
    let provider = provider.as_ref().map(encode).transpose()?;
    insert(db, turn, status, ready, provider, None).await
}

pub(super) async fn inherit(
    db: &Connection,
    turn: &QueuedTurn,
    source: TurnId,
    status: Status,
    ready: bool,
) -> Result<(), Fault> {
    let provider: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT provider FROM agent_runs WHERE turn=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([source.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    insert(
        db,
        turn,
        status,
        ready,
        provider,
        Some(super::super::hooks::frozen(db, source).await?),
    )
    .await
}

async fn insert(
    db: &Connection,
    turn: &QueuedTurn,
    status: Status,
    ready: bool,
    provider: Option<Vec<u8>>,
    hooks: Option<sailry_protocol::hooks::Settings>,
) -> Result<(), Fault> {
    let worktree: String = {
        let mut statement = db
            .prepare("SELECT worktree FROM session_revisions WHERE session=?1 AND revision=?2")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![turn.session.to_string(), turn.revision as i64])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    let hooks = match hooks {
        Some(hooks) => hooks,
        None => super::super::hooks::read(db, worktree.parse().map_err(storage_error)?).await?,
    };
    let run = Run {
        worktree: worktree.parse().map_err(storage_error)?,
        turn: turn.id,
        kind: turn.kind,
        session: turn.session,
        sequence: {
            let mut statement = db
                .prepare("SELECT rowid FROM turns WHERE id=?1")
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row([turn.id.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get::<i64>(0);
            mapped.map_err(storage_error)?
        } as u64,
        revision: turn.revision,
        status,
        error: None,
        started_ms: (status == Status::Running).then(|| chrono::Utc::now().timestamp_millis()),
        finished_ms: None,
        origin: None,
        continuation: None,
        joined: None,
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
    .await
    .map_err(storage_error)?;
    db.execute(
        "INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)",
        params![turn.session.to_string(), turn.id.to_string()],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn get(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_runs WHERE turn=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([turn.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "turn does not exist"))?,
    )
    .map_err(storage_error)
}

pub(in crate::store) async fn visible(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
) -> Result<Run, Fault> {
    let run = get(db, turn).await?;
    let visible: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM conversation_turns WHERE session=?1 AND turn=?2)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![session.to_string(), turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
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

pub(super) async fn put(db: &Connection, run: &Run) -> Result<(), Fault> {
    if run.kind == RunKind::Task
        && matches!(
            run.status,
            Status::Completed | Status::Failed | Status::Interrupted
        )
        && matches!(
            get(db, run.turn).await?.status,
            Status::Queued | Status::Running | Status::Stopping
        )
    {
        super::super::sessions::attention::notify(db, run.session).await?;
    }
    let mut stored = run.clone();
    stored.input = None;
    db.execute(
        "UPDATE agent_runs SET body=?2 WHERE turn=?1",
        params![run.turn.to_string(), encode(&stored)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn list(
    db: &Connection,
    session: SessionId,
    entries: &[Entry],
) -> Result<Vec<Run>, Fault> {
    let turns: Vec<_> = entries.iter().map(|entry| entry.turn).collect();
    let mut query = db.prepare("WITH recent AS (SELECT a.body,t.rowid AS position FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 ORDER BY (json_extract(a.body,'$.status') IN ('running','stopping')) DESC,t.rowid DESC LIMIT 100) SELECT body FROM (SELECT body,position FROM recent UNION SELECT a.body,t.rowid AS position FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND a.turn IN (SELECT value FROM json_each(?2))) ORDER BY position").await.map_err(storage_error)?;
    let runs = {
        let mut rows = query
            .query(params![
                session.to_string(),
                serde_json::to_string(&turns).map_err(storage_error)?
            ])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|body| serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error))
    .collect::<Result<Vec<_>, Fault>>()?;
    Ok(runs)
}

pub(in crate::store) async fn start(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let previous = get(db, turn).await?;
    let run = retry(db, turn).await?;
    if run.status != Status::Queued {
        return Err(Fault::new(ErrorCode::Conflict, "turn is no longer queued"));
    }
    db.execute(
        "UPDATE agent_runs SET ready=1 WHERE turn=?1",
        [turn.to_string()],
    )
    .await
    .map_err(storage_error)?;
    super::queue::bump(db, run.session).await?;
    if previous.status != Status::Queued {
        super::queue::pause(db, run.session, false).await?;
    }
    Ok(run)
}

pub(in crate::store::agent) async fn retry(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let mut run = get(db, turn).await?;
    if run.status == Status::Queued {
        return Ok(run);
    }
    if !super::queue::retained(db, turn).await? {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "turn input has already been consumed",
        ));
    }
    run.status = Status::Queued;
    run.error = None;
    run.started_ms = None;
    run.finished_ms = None;
    put(db, &run).await?;
    Ok(run)
}

pub(in crate::store) async fn stop(db: &Connection, turn: TurnId) -> Result<Run, Fault> {
    let mut run = get(db, turn).await?;
    visible(db, run.session, turn).await?;
    if run.status == Status::Queued || super::queue::retained(db, turn).await? {
        super::background::release(db, turn, true).await?;
        super::queue::remove(db, turn, run.session).await?;
        let mut query = db
            .prepare("SELECT input IS NULL FROM agent_runs WHERE turn=?1")
            .await
            .map_err(storage_error)?;
        let row = query
            .query_row([turn.to_string()])
            .await
            .map_err(storage_error)?;
        let uncaptured: bool = row.get(0).map_err(storage_error)?;
        drop(query);
        if uncaptured {
            super::super::attachments::turns::clear(db, turn).await?;
        }
    } else if matches!(run.status, Status::Running | Status::Stopping) {
        super::queue::pause(db, run.session, true).await?;
        super::queue::guidance::clear(db, run.session, Some(turn)).await?;
    }
    run.status = match run.status {
        Status::Queued => Status::Cancelled,
        Status::Running => Status::Stopping,
        status => status,
    };
    put(db, &run).await?;
    Ok(run)
}

pub(in crate::store) async fn stopping(db: &Connection, turn: TurnId) -> bool {
    get(db, turn)
        .await
        .is_ok_and(|run| matches!(run.status, Status::Stopping | Status::Cancelled))
}

pub(super) async fn claim(
    database: &mut Database,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Invocation>, Fault> {
    claim_with(database, None, events).await
}

pub(super) async fn claim_guidance(
    database: &mut Database,
    recipient: TurnId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Invocation>, Fault> {
    let run = get(&database.connection, recipient).await?;
    if matches!(run.status, Status::Running | Status::Stopping) {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "guidance recipient has not ended",
        ));
    }
    if super::queue::read(&database.connection, run.session)
        .await?
        .paused
    {
        super::queue::guidance::retain(database, recipient, events).await?;
        return Ok(None);
    }
    claim_with(database, Some(recipient), events).await
}

async fn claim_with(
    database: &mut Database,
    recipient: Option<TurnId>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Invocation>, Fault> {
    let publication = database.publication.clone();
    let transaction = database
        .connection
        .unchecked_transaction()
        .await
        .map_err(storage_error)?;
    let mut query = transaction.prepare("SELECT a.body,a.provider,t.caller,r.worktree,p.recipient FROM agent_runs a JOIN agent_pending p ON p.turn=a.turn JOIN turns t ON t.id=a.turn JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision JOIN agent_queues q ON q.session=t.session WHERE a.ready=1 AND q.paused=0 AND json_extract(a.body,'$.status')='queued' AND ((?1 IS NULL AND NOT EXISTS(SELECT 1 FROM agent_delegations d JOIN turns child ON child.id=d.turn WHERE child.session=t.session)) OR (?1 IS NOT NULL AND p.recipient=?1)) AND NOT EXISTS(SELECT 1 FROM agent_runs active JOIN turns other ON other.id=active.turn WHERE other.session=t.session AND json_extract(active.body,'$.status') IN ('running','stopping')) ORDER BY p.position,t.rowid LIMIT 1").await.map_err(storage_error)?;
    let row = (match (async {
        let row = query
            .query_row([recipient.map(|turn| turn.to_string())])
            .await?;
        let mapped: turso::Result<_> = (|row: &turso::Row| {
            Ok((
                row.get::<Vec<u8>>(0)?,
                row.get::<Option<Vec<u8>>>(1)?,
                row.get::<Vec<u8>>(2)?,
                row.get::<String>(3)?,
                row.get::<Option<String>>(4)?,
            ))
        })(&row);
        mapped
    })
    .await
    {
        Ok(value) => Ok(Some(value)),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    })
    .map_err(storage_error)?;
    drop(query);
    let Some((body, provider, caller, worktree, selected)) = row else {
        transaction.rollback().await.map_err(storage_error)?;
        return Ok(None);
    };
    let mut run: Run = serde_json::from_slice(&body).map_err(storage_error)?;
    if let Some(fault) = super::continuation::check(&transaction, &run)
        .await?
        .or(super::background::check(&transaction, &run).await?)
    {
        run = stop(&transaction, run.turn).await?;
        run.error = Some(fault);
        run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        put(&transaction, &run).await?;
        let envelope = changed(&transaction, database.node, run.session).await?;
        let queue = super::queue::read(&transaction, run.session).await?;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        publication.send(events, envelope);
        database
            .feeds
            .publish(database.node, run.session, Change::Queue(queue));
        super::feed::run(database, run).await;
        drop(publication);
        return Box::pin(claim_with(database, recipient, events)).await;
    }
    let queued = super::queue::message(&transaction, run.turn).await?;
    let (followups, joined) = guidance::claim(
        &transaction,
        &run,
        selected
            .map(|turn| turn.parse().map_err(storage_error))
            .transpose()?,
    )
    .await?;
    let plugins =
        crate::store::agent::plugins::packages(&transaction, &queued.turn.plugins).await?;
    let plugin_settings = super::super::plugins::settings::capture(&transaction, &plugins).await;
    let provider: Option<Provider> = provider
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?;
    if let Some(provider) = &provider {
        crate::store::providers::presets::verify(&transaction, provider).await?;
    }
    let media = super::super::media::for_turn(
        super::super::media::session_models(&transaction, queued.turn.session).await?,
        provider.as_ref(),
        &queued.turn.config,
    );
    let background = super::background::bind(&transaction, &mut run).await?;
    let invocation = Invocation {
        instructions: super::super::captures::instructions(&transaction, run.turn).await?,
        notice_input: super::background::render(&transaction, &run.notices).await?,
        notices: run.notices.clone(),
        notice: run.notice,
        hooks: super::super::hooks::frozen(&transaction, queued.turn.id).await?,
        title: super::activity::assigned_title(&transaction, queued.turn.session).await?,
        automatic: {
            let mut statement = transaction
                .prepare("SELECT EXISTS(SELECT 1 FROM agent_continuations WHERE turn=?1)")
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row([queued.turn.id.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        },
        project: {
            let mut statement = transaction
                .prepare("SELECT project FROM worktrees WHERE id=?1")
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row([worktree.as_str()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get::<Option<String>>(0);
            mapped.map_err(storage_error)?
        }
        .map(|id| id.parse().map_err(storage_error))
        .transpose()?,
        connections: connections::catalog(&transaction, queued.turn.session).await?,
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
        input_turn: None,
        followups,
        child: if recipient.is_some() {
            super::delegation::child(&transaction, run.turn).await?
        } else {
            None
        },
    };
    let review_changes = if invocation.child.is_none()
        && !invocation.automatic
        && !run.notice
        && run.kind == RunKind::Task
    {
        super::review::supersede(&transaction, run.session).await?
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
    )
    .await?;
    put(&transaction, &run).await?;
    if run.kind == RunKind::Task {
        super::queue::bump(&transaction, run.session).await?;
    } else {
        super::queue::remove(&transaction, run.turn, run.session).await?;
    }
    let queue = super::queue::read(&transaction, run.session).await?;
    let envelope = changed(&transaction, database.node, run.session).await?;
    let child = super::children::get(&transaction, &run).await?;
    let mut joined_children = Vec::with_capacity(joined.len());
    for joined in joined {
        let child = super::children::get(&transaction, &joined).await?;
        joined_children.push((joined, child));
    }
    let publication = publication.lock().await;
    crate::store::database::conflict::commit(transaction).await?;
    publication.send(events, envelope);
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
    super::feed::run(database, run).await;
    for (run, child) in joined_children {
        super::feed::run(database, run).await;
        if let Some(child) = child {
            database
                .feeds
                .publish(database.node, child.origin.session, Change::Child(child));
        }
    }
    super::background::publish(database, background).await;
    if let Some(child) = child {
        database
            .feeds
            .publish(database.node, child.origin.session, Change::Child(child));
    }
    Ok(Some(invocation))
}

#[cfg(test)]
pub(super) async fn finish(
    database: &mut Database,
    turn: TurnId,
    status: Status,
    error: Option<Fault>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Status, Fault> {
    finish_with(database, turn, status, error, None, events).await
}

pub(super) async fn finish_with(
    database: &mut Database,
    turn: TurnId,
    status: Status,
    error: Option<Fault>,
    review: Option<AdkEvent>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Status, Fault> {
    let publication = database.publication.clone();
    let mut run = get(&database.connection, turn).await?;
    let queued_child = run.started_ms.is_none()
        && run.finished_ms.is_none()
        && matches!(run.status, Status::Queued | Status::Cancelled)
        && matches!(
            status,
            Status::Cancelled | Status::Interrupted | Status::Failed
        )
        && super::delegation::origin(&database.connection, turn)
            .await?
            .is_some();
    if !matches!(run.status, Status::Running | Status::Stopping) && !queued_child {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "turn execution has already ended",
        ));
    }
    let stopped = run.status == Status::Cancelled
        || (run.status == Status::Stopping
            && super::queue::read(&database.connection, run.session)
                .await?
                .paused);
    run.status = if stopped && status != Status::Interrupted {
        Status::Cancelled
    } else {
        status
    };
    if run.status != Status::Completed {
        super::history::retain(database, &run, events).await?;
    }
    let transaction = database
        .connection
        .unchecked_transaction()
        .await
        .map_err(storage_error)?;
    run.error = error;
    run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
    let restored = guidance::restore(&transaction, turn).await?;
    if matches!(
        run.status,
        Status::Failed | Status::Cancelled | Status::Interrupted
    ) {
        let retained = transaction.execute("UPDATE agent_runs SET ready=0 WHERE turn=?1 AND EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=?1)", [turn.to_string()]).await.map_err(storage_error)? > 0;
        let mut query = transaction.prepare("SELECT EXISTS(SELECT 1 FROM agent_events WHERE turn=?1 AND json_extract(body,'$.metadata.compaction.failure') IS NOT NULL AND json_extract(body,'$.metadata.compaction.failure.continued')=0)").await.map_err(storage_error)?;
        let halted: bool = query
            .query_row([turn.to_string()])
            .await
            .map_err(storage_error)?
            .get(0)
            .map_err(storage_error)?;
        drop(query);
        if retained || halted {
            transaction
                .execute(
                    "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE turn=?1",
                    [turn.to_string()],
                )
                .await
                .map_err(storage_error)?;
            super::queue::pause(&transaction, run.session, true).await?;
            super::queue::guidance::clear(&transaction, run.session, Some(turn)).await?;
        }
    }
    if queued_child {
        super::queue::remove(&transaction, turn, run.session).await?;
    }
    put(&transaction, &run).await?;
    let approvals = super::approvals::close(
        &transaction,
        turn,
        if run.status == Status::Interrupted {
            ApprovalState::Interrupted
        } else {
            ApprovalState::Cancelled
        },
    )
    .await?;
    if run.status == Status::Interrupted {
        super::queue::pause(&transaction, run.session, true).await?;
    }
    let questions = super::questions::close(
        &transaction,
        turn,
        if run.status == Status::Interrupted {
            question::State::Interrupted
        } else {
            question::State::Cancelled
        },
    )
    .await?;
    let review_changes = if run.status == Status::Completed {
        (match review {
            Some(value) => Some(
                async {
                    let event = value;
                    super::review::publish(&transaction, &run, event).await
                }
                .await,
            ),
            None => None,
        })
        .transpose()?
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    let queue = super::queue::read(&transaction, run.session).await?;
    let envelope = changed(&transaction, database.node, run.session).await?;
    let child = super::children::get(&transaction, &run).await?;
    let status = run.status;
    let background = super::background::child_finished(&transaction, &run).await?;
    let released = super::background::release(&transaction, turn, run.notice).await?;
    let mut owners: std::collections::BTreeSet<_> =
        background.iter().map(|task| task.session).collect();
    owners.insert(run.session);
    let mut scheduled = Vec::new();
    for owner in owners {
        if let Some(turn) = super::background::schedule(&transaction, database.node, owner).await? {
            scheduled.push(turn);
        }
    }
    let notice = super::super::notifications::system::run(&transaction, &run).await?;
    let notices =
        super::super::database::record(&transaction, database.node, notice.into_iter().collect())
            .await?;
    let mut restored_children = Vec::with_capacity(restored.len());
    for restored in restored {
        let child = super::children::get(&transaction, &restored).await?;
        restored_children.push((restored, child));
    }
    let publication = publication.lock().await;
    crate::store::database::conflict::commit(transaction).await?;
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
    publication.send(events, envelope);
    for notice in notices {
        publication.send(events, notice);
    }
    database
        .approvals
        .lock()
        .unwrap()
        .retain(|_, (owner, _, _, _)| *owner != turn);
    database
        .questions
        .lock()
        .unwrap()
        .retain(|_, (owner, _)| *owner != turn);
    // Publish completion consent before the terminal run so observers cannot
    // conclude the turn without its pending review from the same transaction.
    for (session, change) in review_changes {
        database.feeds.publish(database.node, session, change);
    }
    // The question is pending consent, never a Running or Waiting execution.
    super::feed::run(database, run.clone()).await;
    for (restored, child) in restored_children {
        super::feed::run(database, restored).await;
        if let Some(child) = child {
            database
                .feeds
                .publish(database.node, child.origin.session, Change::Child(child));
        }
    }
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
    super::background::publish(database, background.into_iter().chain(released)).await;
    for turn in scheduled {
        super::feed::run(database, get(&database.connection, turn.id).await?).await;
        let queue = super::queue::read(&database.connection, turn.session).await?;
        database
            .feeds
            .publish(database.node, turn.session, Change::Queue(queue));
        super::background::publish(
            database,
            super::background::list(&database.connection, turn.session).await?,
        )
        .await;
    }
    Ok(status)
}

pub(in crate::store) async fn recover(db: &Connection) -> Result<(), Fault> {
    super::continuation::recover(db).await?;
    super::queue::guidance::recover(db).await?;
    super::delegation::retain_guidance(db, None).await?;
    let mut query = db.prepare("SELECT a.body FROM agent_runs a WHERE json_extract(a.body,'$.status') IN ('running','stopping') OR (json_extract(a.body,'$.status')='queued' AND EXISTS(SELECT 1 FROM agent_delegations d WHERE d.turn=a.turn) AND NOT EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=a.turn))").await.map_err(storage_error)?;
    let rows = {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = row.get::<Vec<u8>>(0);
            values.push(mapped);
        }
        values.into_iter()
    };
    for body in rows {
        let mut run: Run =
            serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
        if run.status == Status::Queued {
            run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        }
        run.status = Status::Interrupted;
        db.execute(
            "UPDATE agent_runs SET ready=0 WHERE turn=?1",
            [run.turn.to_string()],
        )
        .await
        .map_err(storage_error)?;
        let _ = guidance::restore(db, run.turn).await?;
        super::approvals::close(db, run.turn, ApprovalState::Interrupted).await?;
        super::questions::close(db, run.turn, question::State::Interrupted).await?;
        super::queue::pause(db, run.session, true).await?;
        run.error = Some(Fault::new(
            ErrorCode::OutcomeUnknown,
            "execution was interrupted; model and tool calls were not replayed",
        ));
        put(db, &run).await?;
        // Startup has no subscribers yet; the persisted notice joins its first snapshot.
        super::super::notifications::system::run(db, &run).await?;
    }
    super::background::recover(db).await?;
    super::queue::recover(db).await
}
