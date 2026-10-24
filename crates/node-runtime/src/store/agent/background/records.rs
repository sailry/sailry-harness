use super::*;

pub(in crate::store) async fn list(
    db: &Connection,
    session: SessionId,
) -> Result<Vec<Task>, Fault> {
    select(db,"SELECT body FROM agent_background WHERE session=?1 AND state IN ('running','pending') UNION ALL SELECT body FROM (SELECT body FROM agent_background WHERE session=?1 AND state NOT IN ('running','pending') ORDER BY sequence DESC LIMIT 20)",&session.to_string()).await.map(|records|records.into_iter().map(|record|record.task).collect())
}

fn running(
    id: String,
    session: SessionId,
    turn: TurnId,
    kind: Kind,
    title: String,
    started_ms: i64,
) -> Record {
    Record {
        task: Task {
            id,
            session,
            turn,
            kind,
            title,
            revision: 0,
            state: State::Running,
            outcome: None,
            started_ms,
            finished_ms: None,
            summary: None,
            delivery: None,
            recovered: false,
            suppressed: false,
        },
        stdout: None,
        stderr: None,
        response: None,
        elapsed_ms: None,
        sequence: 0,
    }
}

pub(in crate::store) async fn child_started(
    db: &Connection,
    parent: &Run,
    child: &Run,
    title: &str,
) -> Result<Task, Fault> {
    let id = format!("agent:{}", child.turn);
    if let Some(record) = get(db, &id).await? {
        return Ok(record.task);
    }
    let mut record = running(
        id,
        parent.session,
        parent.turn,
        Kind::Agent {
            session: child.session,
            turn: child.turn,
        },
        preview(title, 200),
        child.started_ms.unwrap_or_else(now),
    );
    save(db, &mut record).await?;
    Ok(record.task)
}

pub(in crate::store) async fn command_started(
    db: &Connection,
    request: &Request,
) -> Result<Option<Task>, Fault> {
    let Command::RunCommand {
        turn,
        command,
        description,
        background: true,
        ..
    } = &request.command
    else {
        return Ok(None);
    };
    let id = format!("command:{}", request.id);
    let run = runs::get(db, *turn).await?;
    if let Some(record) = get(db, &id).await? {
        if record.task.turn != *turn || record.task.session != run.session {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "command identity belongs to another execution",
            ));
        }
        return Ok(Some(record.task));
    }
    let mut record = running(
        id,
        run.session,
        *turn,
        Kind::Command { id: request.id },
        preview(description.as_deref().unwrap_or(command), 200),
        now(),
    );
    save(db, &mut record).await?;
    Ok(Some(record.task))
}

async fn terminal(
    db: &Connection,
    record: &mut Record,
    outcome: Outcome,
    summary: Option<String>,
) -> Result<(), Fault> {
    record.task.state = State::Pending;
    record.task.outcome = Some(outcome);
    record.task.finished_ms = Some(now());
    record.task.summary = summary.map(|text| preview(&text, 512));
    record.sequence = next_sequence(db).await?;
    save(db, record).await
}

pub(in crate::store) async fn child_finished(
    db: &Connection,
    run: &Run,
) -> Result<Vec<Task>, Fault> {
    if matches!(
        run.status,
        Status::Running | Status::Stopping | Status::Queued | Status::Continued
    ) {
        return Ok(Vec::new());
    }
    if queue::read(db, run.session)
        .await?
        .items
        .iter()
        .any(|item| item.guidance.is_some())
    {
        return Ok(Vec::new());
    }
    let mut changed = Vec::new();
    for mut record in select(
        db,
        "SELECT body FROM agent_background WHERE kind='agent' AND source=?1 AND state='running'",
        &run.session.to_string(),
    )
    .await?
    {
        record.task.kind = Kind::Agent {
            session: run.session,
            turn: run.turn,
        };
        record.response = super::super::delegation::control::response(db, run.turn)
            .await?
            .map(|text| preview(&text, 4096));
        let outcome = match run.status {
            Status::Completed => Outcome::Completed,
            Status::Failed => Outcome::Failed,
            Status::Cancelled => Outcome::Cancelled,
            _ => Outcome::Interrupted,
        };
        let summary = record
            .response
            .clone()
            .or_else(|| run.error.as_ref().map(|error| error.message.clone()));
        terminal(db, &mut record, outcome, summary).await?;
        changed.push(record.task);
    }
    Ok(changed)
}

pub(in crate::store) async fn command_finished(
    db: &Connection,
    snapshot: &process::Snapshot,
    elapsed_ms: Option<u64>,
    explicit: bool,
) -> Result<Option<Task>, Fault> {
    let id = format!("command:{}", snapshot.info.id);
    let Some(mut record) = get(db, &id).await? else {
        return Err(storage_error(
            "background command was not registered before execution",
        ));
    };
    if record.task.session != snapshot.info.session || record.task.turn != snapshot.info.turn {
        return Err(storage_error("background command ownership changed"));
    }
    if record.task.state != State::Running {
        return Ok(None);
    }
    let outcome = match &snapshot.info.status {
        process::Status::Running | process::Status::Stopping => return Ok(None),
        process::Status::Finished(process::Outcome::Exited(0)) => Outcome::Completed,
        process::Status::Finished(process::Outcome::TimedOut) => Outcome::TimedOut,
        process::Status::Finished(process::Outcome::Cancelled) => {
            if explicit {
                Outcome::Stopped
            } else {
                Outcome::Cancelled
            }
        }
        process::Status::Finished(process::Outcome::Unknown(_)) => Outcome::Unknown,
        process::Status::Finished(_) | process::Status::Failed(_) => Outcome::Failed,
    };
    record.stdout = Some(tail(&snapshot.stdout.text, 2048));
    record.stderr = Some(tail(&snapshot.stderr.text, 2048));
    record.elapsed_ms = elapsed_ms;
    let summary = if !snapshot.stderr.text.is_empty() {
        Some(tail(&snapshot.stderr.text, 512))
    } else if !snapshot.stdout.text.is_empty() {
        Some(tail(&snapshot.stdout.text, 512))
    } else {
        match &snapshot.info.status {
            process::Status::Failed(message) => Some(message.clone()),
            _ => None,
        }
    };
    terminal(db, &mut record, outcome, summary).await?;
    Ok(Some(record.task))
}

pub(in crate::store) async fn consume_command(
    db: &Connection,
    parent: TurnId,
    snapshot: &process::Snapshot,
) -> Result<Vec<Task>, Fault> {
    let run = calls::active(db, parent).await?;
    if run.session != snapshot.info.session {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "command belongs to another session",
        ));
    }
    if matches!(
        snapshot.info.status,
        process::Status::Running | process::Status::Stopping
    ) {
        return Ok(Vec::new());
    }
    // The snapshot has already been read by the tool. Completing it here closes
    // the terminal-read/sink race on the same storage actor without consuming UI polls.
    let _ = command_finished(db, snapshot, None, false).await?;
    consume(db, &format!("command:{}", snapshot.info.id), run.session).await
}

pub(in crate::store) async fn consume_child(
    db: &Connection,
    parent: TurnId,
    child: SessionId,
    turn: TurnId,
) -> Result<Vec<Task>, Fault> {
    let owner = calls::active(db, parent).await?.session;
    let mut changed = Vec::new();
    for record in select(
        db,
        "SELECT body FROM agent_background WHERE kind='agent' AND source=?1",
        &child.to_string(),
    )
    .await?
    {
        if record.task.session == owner
            && matches!(record.task.kind,Kind::Agent{turn:source,..} if source==turn)
            && record.task.state != State::Running
        {
            changed.extend(consume(db, &record.task.id, owner).await?);
        }
    }
    Ok(changed)
}

async fn consume(db: &Connection, id: &str, owner: SessionId) -> Result<Vec<Task>, Fault> {
    let Some(mut record) = get(db, id).await? else {
        return Ok(Vec::new());
    };
    if record.task.session != owner {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "completion belongs to another session",
        ));
    }
    if matches!(record.task.state, State::Consumed | State::Dismissed) {
        return Ok(Vec::new());
    }
    record.task.state = State::Consumed;
    record.task.delivery = None;
    save(db, &mut record).await?;
    Ok(vec![record.task])
}

pub(in crate::store) async fn release(
    db: &Connection,
    turn: TurnId,
    suppress: bool,
) -> Result<Vec<Task>, Fault> {
    let mut tasks = Vec::new();
    for mut record in select(
        db,
        "SELECT body FROM agent_background WHERE state='pending' AND delivery=?1",
        &turn.to_string(),
    )
    .await?
    {
        record.task.delivery = None;
        record.task.suppressed |= suppress;
        save(db, &mut record).await?;
        tasks.push(record.task);
    }
    Ok(tasks)
}

pub(in crate::store) async fn dismiss(db: &Connection, session: SessionId) -> Result<(), Fault> {
    for mut record in select(
        db,
        "SELECT body FROM agent_background WHERE session=?1 AND state IN ('running','pending')",
        &session.to_string(),
    )
    .await?
    {
        record.task.state = State::Dismissed;
        record.task.delivery = None;
        save(db, &mut record).await?;
    }
    Ok(())
}

pub(in crate::store) async fn recover(db: &Connection) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT body FROM agent_background WHERE state IN ('running','pending')")
        .await
        .map_err(storage_error)?;
    let records = (async {
        let mut rows = query.query(()).await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .map_err(storage_error)?
    .map(|body| {
        serde_json::from_slice::<Record>(&body.map_err(storage_error)?).map_err(storage_error)
    })
    .collect::<Result<Vec<_>, Fault>>()?;
    drop(query);
    for mut record in records {
        if record.task.state == State::Running {
            terminal(
                db,
                &mut record,
                Outcome::Interrupted,
                Some("Execution was interrupted; its outcome was not confirmed or replayed".into()),
            )
            .await?;
        }
        if let Some(turn) = record.task.delivery {
            let mut run = runs::get(db, turn).await?;
            if run.notice && run.status == Status::Queued {
                run.status = Status::Cancelled;
                runs::put(db, &run).await?;
                queue::remove(db, turn, run.session).await?;
            }
        }
        record.task.delivery = None;
        record.task.recovered = true;
        record.task.suppressed = true;
        save(db, &mut record).await?;
    }
    Ok(())
}

pub(in crate::store) async fn finish_request(
    db: &Connection,
    request: &Request,
    result: &Result<Output, Fault>,
) -> Result<Option<Task>, Fault> {
    let Command::RunCommand {
        background: true, ..
    } = request.command
    else {
        return Ok(None);
    };
    if matches!(result, Ok(Output::CommandStarted(_))) {
        return Ok(None);
    }
    let Some(mut record) = get(db, &format!("command:{}", request.id)).await? else {
        return Ok(None);
    };
    if record.task.state != State::Running {
        return Ok(None);
    }
    let (outcome, summary) = match result {
        Ok(Output::CommandResult(result)) => {
            record.stdout = Some(tail(&result.stdout.text, 2048));
            record.stderr = Some(tail(&result.stderr.text, 2048));
            record.elapsed_ms = Some(result.elapsed_ms);
            (
                match result.outcome {
                    process::Outcome::Cancelled => Outcome::Cancelled,
                    process::Outcome::TimedOut => Outcome::TimedOut,
                    process::Outcome::Unknown(_) => Outcome::Unknown,
                    process::Outcome::Exited(0) => Outcome::Completed,
                    _ => Outcome::Failed,
                },
                None,
            )
        }
        Err(error) => (
            match error.code {
                ErrorCode::Cancelled => Outcome::Cancelled,
                ErrorCode::OutcomeUnknown => Outcome::Unknown,
                _ => Outcome::Failed,
            },
            Some(error.message.clone()),
        ),
        _ => {
            return Err(storage_error(
                "background start returned an unexpected receipt",
            ));
        }
    };
    terminal(db, &mut record, outcome, summary).await?;
    Ok(Some(record.task))
}
