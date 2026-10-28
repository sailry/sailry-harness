//! Pending input points into the existing command ledger; ADK owns executed history.
use super::*;

mod commands;
pub(in crate::store) mod guidance;
#[cfg(test)]
mod tests;
pub(in crate::store) use commands::execute;

const CAPACITY: i64 = 256;
const PREVIEW_CHARS: usize = 256;

pub(in crate::store) async fn create(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    session: sailry_protocol::Session,
    message: &Input,
    ready: bool,
) -> Result<QueuedTurn, Fault> {
    validate(message)?;
    super::references::validate(db, &session, message, None).await?;
    let turn = capture(db, caller, request, session, RunKind::Task, ready).await?;
    super::super::attachments::turns::bind(db, turn.id, &message.attachments).await?;
    Ok(turn)
}

pub(in crate::store) async fn compact(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    session: sailry_protocol::Session,
) -> Result<QueuedTurn, Fault> {
    input(request.command.clone())?;
    capture(db, caller, request, session, RunKind::Compaction, true).await
}

pub(in crate::store) async fn notice(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    session: sailry_protocol::Session,
) -> Result<QueuedTurn, Fault> {
    capture(db, caller, request, session, RunKind::Task, true).await
}

async fn capture(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    session: sailry_protocol::Session,
    kind: RunKind,
    ready: bool,
) -> Result<QueuedTurn, Fault> {
    super::super::sessions::writable(&session)?;
    super::super::worktrees::present(db, session.worktree).await?;
    super::super::commands::validate_config(db, request.target, &session.config).await?;
    let plugins = super::plugins::configuration(db, &session.config).await?;
    let mut roles = session.roles;
    if session.profile.is_none() {
        super::super::sessions::roles::refresh(db, &mut roles).await?;
    }
    let turn = QueuedTurn {
        id: TurnId::new(),
        kind,
        session: session.id,
        request: request.id,
        revision: session.revision,
        config: session.config,
        roles,
        plugins,
    };
    // The request ledger owns input; this row only freezes its configuration.
    db.execute(
        "INSERT INTO turns(id,session,revision,request,caller,kind,plugins,roles,rowid) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            turn.id.to_string(),
            turn.session.to_string(),
            turn.revision as i64,
            request.id.to_string(),
            &caller.0[..],
            match kind {
                RunKind::Task => "task",
                RunKind::Compaction => "compaction",
            },
            encode(&turn.plugins)?,
            encode(&turn.roles)?, crate::store::database::counters::next(db, crate::store::database::counters::Namespace::Turns).await? as i64],
    ).await
    .map_err(storage_error)?;
    super::super::plugins::connectors::capture(db, turn.id, &turn.plugins).await?;
    super::admit(db, &turn, ready).await?;
    Ok(turn)
}

pub(in crate::store) async fn can_resume(
    db: &Connection,
    session: SessionId,
) -> Result<bool, Fault> {
    Ok(read(db, session).await?.paused
        && !runs::list(db, session, &[])
            .await?
            .iter()
            .any(|run| run.status == Status::Running))
}

pub(in crate::store) async fn prioritize(db: &Connection, turn: &QueuedTurn) -> Result<(), Fault> {
    let first = read(db, turn.session)
        .await?
        .items
        .first()
        .map(|item| item.turn);
    reorder(db, turn.session, turn.id, first).await?;
    pause(db, turn.session, false).await
}

pub(super) async fn admit(db: &Connection, turn: &QueuedTurn, ready: bool) -> Result<(), Fault> {
    let count: i64 = {
        let mut statement = db.prepare("SELECT count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1").await.map_err(storage_error)?;
        let row = statement
            .query_row([turn.session.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if count >= CAPACITY {
        return Err(Fault::new(ErrorCode::Busy, "session queue is full"));
    }
    db.execute(
        "INSERT INTO agent_pending(turn,position,revision,request,caller) SELECT id,(SELECT coalesce(max(p.position),0)+1 FROM agent_pending p JOIN turns other ON other.id=p.turn WHERE other.session=t.session),1,request,caller FROM turns t WHERE id=?1",
        [turn.id.to_string()],
    ).await.map_err(storage_error)?;
    bump(db, turn.session).await?;
    if ready {
        // Fresh submitted input resumes the queue in the same admission transaction.
        db.execute(
            "UPDATE agent_queues SET paused=0 WHERE session=?1",
            [turn.session.to_string()],
        )
        .await
        .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) async fn bump(db: &Connection, session: SessionId) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO agent_queues(session,revision,paused) VALUES(?1,1,0) ON CONFLICT(session) DO UPDATE SET revision=revision+1",
        [session.to_string()],
    ).await.map_err(storage_error)?;
    Ok(())
}

pub(super) async fn pause(db: &Connection, session: SessionId, paused: bool) -> Result<(), Fault> {
    bump(db, session).await?;
    db.execute(
        "UPDATE agent_queues SET paused=?2 WHERE session=?1",
        params![session.to_string(), paused],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn recover(db: &Connection) -> Result<(), Fault> {
    // Restart requires an explicit resume even when no worker claimed the input.
    db.execute(
        "UPDATE agent_queues SET paused=1,revision=revision+1 WHERE paused=0 AND EXISTS(SELECT 1 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=agent_queues.session)",
        (),
    ).await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn remove(db: &Connection, turn: TurnId, session: SessionId) -> Result<(), Fault> {
    if db
        .execute(
            "DELETE FROM agent_pending WHERE turn=?1",
            [turn.to_string()],
        )
        .await
        .map_err(storage_error)?
        > 0
    {
        bump(db, session).await?;
    }
    Ok(())
}

pub(super) async fn read(db: &Connection, session: SessionId) -> Result<Queue, Fault> {
    check_session(db, session).await?;
    let (revision, paused) = {
        let mut statement = db
            .prepare("SELECT revision,paused FROM agent_queues WHERE session=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([session.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get::<i64>(0)? as u64, row.get(1)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    }
    .unwrap_or((0, false));
    // Bound previews, not user input. Editing fetches the complete original text separately.
    let mut query = db.prepare(
        "SELECT a.body,p.revision,a.ready,r.body,p.recipient FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN agent_runs a ON a.turn=p.turn JOIN requests r ON r.id=p.request AND r.caller=p.caller WHERE t.session=?1 AND (json_extract(a.body,'$.status')='queued' OR (json_extract(a.body,'$.status') IN ('failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=p.turn AND json_extract(e.body,'$.provenance.author')='user'))) ORDER BY p.position,t.rowid"
    ).await.map_err(storage_error)?;
    let mut rows = query
        .query([session.to_string()])
        .await
        .map_err(storage_error)?;
    let mut items = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body = row.get::<Vec<u8>>(0).map_err(storage_error)?;
        let revision = row.get::<i64>(1).map_err(storage_error)? as u64;
        let ready = row.get::<bool>(2).map_err(storage_error)?;
        let request = row.get::<Vec<u8>>(3).map_err(storage_error)?;
        let recipient = row.get::<Option<String>>(4).map_err(storage_error)?;
        let request: Request = serde_json::from_slice(&request).map_err(storage_error)?;
        let mut preview = input(request.command)?.text;
        let run: Run = serde_json::from_slice(&body).map_err(storage_error)?;
        let end = preview
            .char_indices()
            .nth(PREVIEW_CHARS)
            .map_or(preview.len(), |(index, _)| index);
        let truncated = end < preview.len();
        preview.truncate(end);
        items.push(Pending {
            turn: run.turn,
            notice: run.notice,
            request: Some(request.id),
            kind: run.kind,
            revision,
            config_revision: run.revision,
            ready,
            guidance: recipient
                .map(|id| id.parse().map_err(storage_error))
                .transpose()?,
            preview,
            truncated,
            attachments: super::super::attachments::turns::list(db, run.turn).await?,
        });
    }
    Ok(Queue {
        revision,
        paused,
        items,
    })
}

pub(in crate::store) async fn message(
    db: &Connection,
    turn: TurnId,
) -> Result<QueuedMessage, Fault> {
    let run = runs::get(db, turn).await?;
    if run.status != Status::Queued && !retained(db, turn).await? {
        return Err(Fault::new(ErrorCode::Conflict, "turn is no longer queued"));
    }
    let (revision, body, config, request): (u64, Vec<u8>, Vec<u8>, String) = {
        let mut statement = db.prepare("SELECT p.revision,r.body,c.config,t.request FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN requests r ON r.id=p.request AND r.caller=p.caller JOIN session_revisions c ON c.session=t.session AND c.revision=t.revision WHERE p.turn=?1").await.map_err(storage_error)?;
        let row = statement
            .query_row([turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| {
            Ok((
                row.get::<i64>(0)? as u64,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        })(&row);
        mapped.map_err(storage_error)?
    };
    let request_body: Request = serde_json::from_slice(&body).map_err(storage_error)?;
    let message = input(request_body.command)?;
    Ok(QueuedMessage {
        turn: QueuedTurn {
            id: turn,
            kind: run.kind,
            session: run.session,
            revision: run.revision,
            request: request.parse().map_err(storage_error)?,
            config: serde_json::from_slice(&config).map_err(storage_error)?,
            roles: super::super::sessions::roles::for_turn(db, run.turn).await?,
            plugins: super::plugins::read(db, run.turn).await?,
        },
        revision,
        message,
    })
}

fn input(command: Command) -> Result<Input, Fault> {
    match command {
        Command::PluginTransaction { operations } => operations
            .into_iter()
            .find_map(|operation| match operation {
                plugin::transaction::Operation::Submit { message, .. }
                | plugin::transaction::Operation::Continue { message, .. } => Some(message),
                _ => None,
            })
            .ok_or_else(|| storage_error("transaction has no pending input")),
        Command::StartSession(draft) => Ok(draft.message),
        Command::QueueTurn { message, .. }
        | Command::ReplaceTurn { message, .. }
        | Command::SubmitTurn { message, .. }
        | Command::SubmitTask { message, .. }
        | Command::GuideTurn { message, .. }
        | Command::ContinueTurn { message, .. }
        | Command::EditQueuedTurn { message, .. } => Ok(message),
        Command::ResolveQuestion {
            response: question::Response::StartCoding { message, .. },
            ..
        } => Ok(message),
        Command::CompactContext { focus, .. } => {
            let text = focus.unwrap_or_default();
            if text.len() > 64 * 1024 {
                return Err(Fault::invalid("context compaction focus is too long"));
            }
            Ok(Input {
                text,
                ..Default::default()
            })
        }
        Command::DeliverBackground { .. } => Ok(Input::default()),
        _ => Err(storage_error("pending input is not a message")),
    }
}

pub(in crate::store) fn validate(message: &Input) -> Result<(), Fault> {
    if message.is_empty()
        || message.text.len() > 1024 * 1024
        || message.references.len() > 32
        || message.attachments.len() > 8
        || message
            .attachments
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != message.attachments.len()
    {
        Err(sailry_protocol::Fault::invalid(
            "input must be nonempty with at most one MiB of text, eight distinct attachments and 32 references",
        ))
    } else {
        Ok(())
    }
}

fn check_revision(actual: u64, expected: u64) -> Result<(), Fault> {
    if actual == expected {
        Ok(())
    } else {
        Err(Fault::new(
            ErrorCode::RevisionConflict,
            "queue changed; reload before editing",
        ))
    }
}

async fn check_item(db: &Connection, turn: TurnId, expected: u64) -> Result<Run, Fault> {
    let run = runs::get(db, turn).await?;
    if run.status != Status::Queued && !retained(db, turn).await? {
        return Err(Fault::new(ErrorCode::Conflict, "turn is no longer queued"));
    }
    let revision = {
        let mut statement = db
            .prepare("SELECT revision FROM agent_pending WHERE turn=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| Ok(row.get::<i64>(0)? as u64))(&row);
        mapped.map_err(storage_error)?
    };
    check_revision(revision, expected)?;
    Ok(run)
}

pub(in crate::store::agent) async fn retained(
    db: &Connection,
    turn: TurnId,
) -> Result<bool, Fault> {
    let mut query = db.prepare("SELECT EXISTS(SELECT 1 FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn WHERE p.turn=?1 AND json_extract(a.body,'$.status') IN ('failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_events e WHERE e.turn=p.turn AND json_extract(e.body,'$.provenance.author')='user'))").await.map_err(storage_error)?;
    let row = query
        .query_row([turn.to_string()])
        .await
        .map_err(storage_error)?;
    row.get(0).map_err(storage_error)
}

async fn reorder(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    before: Option<TurnId>,
) -> Result<(), Fault> {
    let mut items = read(db, session).await?.items;
    let source = items
        .iter()
        .position(|item| item.turn == turn)
        .ok_or_else(|| {
            sailry_protocol::Fault::invalid(
                "queued turn belongs to another session or has already started",
            )
        })?;
    if before == Some(turn) {
        return Ok(());
    }
    let item = items.remove(source);
    let target = match before {
        Some(before) => items
            .iter()
            .position(|item| item.turn == before)
            .ok_or_else(|| {
                sailry_protocol::Fault::invalid(
                    "queue destination no longer exists in this session",
                )
            })?,
        None => items.len(),
    };
    items.insert(target, item);
    for (position, item) in items.iter().enumerate() {
        db.execute(
            "UPDATE agent_pending SET position=?2 WHERE turn=?1",
            params![item.turn.to_string(), position as i64 + 1],
        )
        .await
        .map_err(storage_error)?;
    }
    bump(db, session).await
}

pub(in crate::store) async fn stopping(
    db: &Connection,
    selected: TurnId,
) -> Result<Vec<TurnId>, Fault> {
    let mut query = db
        .prepare("SELECT a.turn FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=(SELECT session FROM turns WHERE id=?1) AND json_extract(a.body,'$.status')='stopping'").await
        .map_err(storage_error)?;
    (async {
        let mut rows = query.query([selected.to_string()]).await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> = row.get::<String>(0);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
    .map_err(storage_error)?
    .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
    .collect()
}
