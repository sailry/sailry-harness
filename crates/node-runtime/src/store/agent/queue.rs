//! Pending input points into the existing command ledger; ADK owns executed history.
use super::*;

mod commands;
pub(in crate::store) use commands::execute;

const CAPACITY: i64 = 256;
const PREVIEW_CHARS: usize = 256;

pub(in crate::store) fn create(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    session: sailry_protocol::Session,
    message: &Input,
    ready: bool,
) -> Result<QueuedTurn, Fault> {
    validate(message)?;
    super::references::validate(db, &session, message, None)?;
    let turn = capture(db, caller, request, session, RunKind::Task, ready)?;
    super::super::attachments::turns::bind(db, turn.id, &message.attachments)?;
    Ok(turn)
}

pub(in crate::store) fn compact(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    session: sailry_protocol::Session,
) -> Result<QueuedTurn, Fault> {
    capture(db, caller, request, session, RunKind::Compaction, true)
}

fn capture(
    db: &Connection,
    caller: NodeId,
    request: &Request,
    session: sailry_protocol::Session,
    kind: RunKind,
    ready: bool,
) -> Result<QueuedTurn, Fault> {
    super::super::sessions::writable(&session)?;
    super::super::commands::validate_config(db, request.target, &session.config)?;
    let plugins = if let Some(binding) = &session.config.assistant {
        super::plugins::assistant(db, binding)?
    } else {
        super::plugins::capture(db)?
    };
    let turn = QueuedTurn {
        id: TurnId::new(),
        kind,
        session: session.id,
        request: request.id,
        revision: session.revision,
        config: session.config,
        roles: session.roles,
        plugins,
    };
    // The request ledger owns input; this row only freezes its configuration.
    db.execute(
        "INSERT INTO turns(id,session,revision,request,caller,kind,plugins) VALUES(?1,?2,?3,?4,?5,?6,?7)",
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
        ],
    )
    .map_err(storage_error)?;
    super::admit(db, &turn, ready)?;
    Ok(turn)
}

pub(super) fn prioritize(db: &Connection, turn: &QueuedTurn) -> Result<(), Fault> {
    let first = read(db, turn.session)?.items.first().map(|item| item.turn);
    reorder(db, turn.session, turn.id, first)?;
    pause(db, turn.session, false)
}

pub(super) fn admit(db: &Connection, turn: &QueuedTurn, ready: bool) -> Result<(), Fault> {
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1",
            [turn.session.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if count >= CAPACITY {
        return Err(Fault::new(ErrorCode::Busy, "session queue is full"));
    }
    db.execute(
        "INSERT INTO agent_pending(turn,position,revision,request,caller) SELECT id,(SELECT coalesce(max(p.position),0)+1 FROM agent_pending p JOIN turns other ON other.id=p.turn WHERE other.session=t.session),1,request,caller FROM turns t WHERE id=?1",
        [turn.id.to_string()],
    ).map_err(storage_error)?;
    bump(db, turn.session)?;
    if ready {
        // Explicit input resumes an idle/stopping session; background completion never does.
        db.execute(
            "UPDATE agent_queues SET paused=0 WHERE session=?1 AND NOT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status')='running')",
            [turn.session.to_string()],
        ).map_err(storage_error)?;
    }
    Ok(())
}

pub(super) fn bump(db: &Connection, session: SessionId) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO agent_queues(session,revision,paused) VALUES(?1,1,0) ON CONFLICT(session) DO UPDATE SET revision=revision+1",
        [session.to_string()],
    ).map_err(storage_error)?;
    Ok(())
}

pub(super) fn pause(db: &Connection, session: SessionId, paused: bool) -> Result<(), Fault> {
    bump(db, session)?;
    db.execute(
        "UPDATE agent_queues SET paused=?2 WHERE session=?1",
        params![session.to_string(), paused],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn remove(db: &Connection, turn: TurnId, session: SessionId) -> Result<(), Fault> {
    if db
        .execute(
            "DELETE FROM agent_pending WHERE turn=?1",
            [turn.to_string()],
        )
        .map_err(storage_error)?
        > 0
    {
        bump(db, session)?;
    }
    Ok(())
}

pub(super) fn read(db: &Connection, session: SessionId) -> Result<Queue, Fault> {
    check_session(db, session)?;
    let (revision, paused) = db
        .query_row(
            "SELECT revision,paused FROM agent_queues WHERE session=?1",
            [session.to_string()],
            |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?
        .unwrap_or((0, false));
    // Bound previews, not user input. Editing fetches the complete original text separately.
    let mut query = db.prepare(
        "SELECT a.body,p.revision,a.ready,r.body FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN agent_runs a ON a.turn=p.turn JOIN requests r ON r.id=p.request AND r.caller=p.caller WHERE t.session=?1 ORDER BY p.position,t.rowid"
    ).map_err(storage_error)?;
    let items = query
        .query_map([session.to_string()], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (body, revision, ready, request) = row.map_err(storage_error)?;
            let request: Request = serde_json::from_slice(&request).map_err(storage_error)?;
            let mut preview = input(request.command)?.text;
            let run: Run = serde_json::from_slice(&body).map_err(storage_error)?;
            let end = preview
                .char_indices()
                .nth(PREVIEW_CHARS)
                .map_or(preview.len(), |(index, _)| index);
            let truncated = end < preview.len();
            preview.truncate(end);
            Ok(Pending {
                turn: run.turn,
                kind: run.kind,
                revision,
                config_revision: run.revision,
                ready,
                preview,
                truncated,
                attachments: super::super::attachments::turns::list(db, run.turn)?,
            })
        })
        .collect::<Result<_, Fault>>()?;
    Ok(Queue {
        revision,
        paused,
        items,
    })
}

pub(in crate::store) fn message(db: &Connection, turn: TurnId) -> Result<QueuedMessage, Fault> {
    let run = runs::get(db, turn)?;
    if run.status != Status::Queued {
        return Err(Fault::new(ErrorCode::Conflict, "turn is no longer queued"));
    }
    let (revision, body, config, request): (u64, Vec<u8>, Vec<u8>, String) = db.query_row(
        "SELECT p.revision,r.body,c.config,t.request FROM agent_pending p JOIN turns t ON t.id=p.turn JOIN requests r ON r.id=p.request AND r.caller=p.caller JOIN session_revisions c ON c.session=t.session AND c.revision=t.revision WHERE p.turn=?1",
        [turn.to_string()], |row| Ok((row.get::<_, i64>(0)? as u64,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).map_err(storage_error)?;
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
            roles: super::super::sessions::roles::read(db, run.session, run.revision)?,
            plugins: super::plugins::read(db, run.turn)?,
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
        | Command::ContinueTurn { message, .. }
        | Command::EditQueuedTurn { message, .. } => Ok(message),
        Command::ResolveQuestion {
            response: question::Response::StartCoding { message, .. },
            ..
        } => Ok(message),
        Command::CompactContext { .. } => Ok(Input::default()),
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
        Err(invalid(
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

fn check_item(db: &Connection, turn: TurnId, expected: u64) -> Result<Run, Fault> {
    let run = runs::get(db, turn)?;
    if run.status != Status::Queued {
        return Err(Fault::new(ErrorCode::Conflict, "turn is no longer queued"));
    }
    let revision = db
        .query_row(
            "SELECT revision FROM agent_pending WHERE turn=?1",
            [turn.to_string()],
            |row| Ok(row.get::<_, i64>(0)? as u64),
        )
        .map_err(storage_error)?;
    check_revision(revision, expected)?;
    Ok(run)
}

fn reorder(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    before: Option<TurnId>,
) -> Result<(), Fault> {
    let mut items = read(db, session)?.items;
    let source = items
        .iter()
        .position(|item| item.turn == turn)
        .ok_or_else(|| invalid("queued turn belongs to another session or has already started"))?;
    if before == Some(turn) {
        return Ok(());
    }
    let item = items.remove(source);
    let target = match before {
        Some(before) => items
            .iter()
            .position(|item| item.turn == before)
            .ok_or_else(|| invalid("queue destination no longer exists in this session"))?,
        None => items.len(),
    };
    items.insert(target, item);
    for (position, item) in items.iter().enumerate() {
        db.execute(
            "UPDATE agent_pending SET position=?2 WHERE turn=?1",
            params![item.turn.to_string(), position as i64 + 1],
        )
        .map_err(storage_error)?;
    }
    bump(db, session)
}

pub(in crate::store) fn stopping(db: &Connection, selected: TurnId) -> Result<Vec<TurnId>, Fault> {
    let mut query = db
        .prepare("SELECT a.turn FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=(SELECT session FROM turns WHERE id=?1) AND json_extract(a.body,'$.status')='stopping'")
        .map_err(storage_error)?;
    query
        .query_map([selected.to_string()], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
        .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
        .collect()
}
