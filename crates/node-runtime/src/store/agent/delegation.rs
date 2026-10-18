//! Child admission references a parent call and reuses the sole session history path.
use super::*;
use adk_core::ToolConfirmationRequest;
use serde::Deserialize;
use serde_json::Value;

pub(super) mod control;
mod fresh;
mod guidance;
mod reuse;
pub(crate) use control::State;
pub(in crate::store) use control::owned;
pub(in crate::store) use guidance::{guide, queued_guidance, retain_guidance};

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod statistics_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod worktree_tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    session: Option<SessionId>,
    role: Option<String>,
    title: Option<String>,
    task: String,
    worktree: Option<WorktreeId>,
    #[serde(default, rename = "background")]
    _background: bool,
}

impl Ingress {
    pub(crate) async fn delegate(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
        arguments: Value,
        message: Option<Input>,
        token: sailry_link::CancellationToken,
    ) -> Result<Invocation, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Delegate {
                turn,
                request,
                arguments,
                message,
                token,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}

pub(in crate::store) fn read(
    db: &Connection,
    session: SessionId,
) -> Result<Option<Box<Delegation>>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT d.body FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=?1 ORDER BY t.rowid LIMIT 1",
            [session.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(in crate::store) fn read_only(db: &Connection, session: SessionId) -> Result<bool, Fault> {
    let turn: String = db.query_row(
        "SELECT t.id FROM turns t JOIN agent_delegations d ON d.turn=t.id WHERE t.session=?1 ORDER BY t.rowid LIMIT 1",
        [session.to_string()],
        |row| row.get(0),
    ).map_err(storage_error)?;
    let turn = turn.parse().map_err(storage_error)?;
    Ok(super::permissions::config(db, turn)?.mode == WorkMode::Plan)
}

pub(super) fn origin(db: &Connection, turn: TurnId) -> Result<Option<Delegation>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_delegations WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(in crate::store) fn child(db: &Connection, turn: TurnId) -> Result<Option<Child>, Fault> {
    let Some(origin) = origin(db, turn)? else {
        return Ok(None);
    };
    let run = runs::get(db, turn)?;
    let founding = read(db, run.session)?
        .ok_or_else(|| storage_error("child session has no founding delegation"))?;
    if origin.session != founding.session || origin.role != founding.role {
        return Err(storage_error(
            "child ownership differs from its founding delegation",
        ));
    }
    let role = if let Some(id) = founding.role {
        let parent = runs::get(db, founding.turn)?;
        let roster = super::super::sessions::roles::read(db, parent.session, parent.revision)?;
        Some(
            roster
                .profiles
                .into_iter()
                .find(|role| role.id == id)
                .ok_or_else(|| storage_error("frozen child role is missing"))?,
        )
    } else {
        None
    };
    Ok(Some(Child { role }))
}

fn insert(db: &Connection, turn: TurnId, origin: &Delegation) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO agent_delegations(turn,parent_turn,entry,part,body) VALUES(?1,?2,?3,?4,?5)",
        params![
            turn.to_string(),
            origin.turn.to_string(),
            origin.entry,
            origin.index as i64,
            encode(origin)?
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn follow(
    db: &Connection,
    recipient: TurnId,
    source: TurnId,
) -> Result<(), Fault> {
    let origin = origin(db, recipient)?
        .ok_or_else(|| Fault::new(ErrorCode::WrongTarget, "guidance recipient is not a child"))?;
    if runs::get(db, recipient)?.session != runs::get(db, source)?.session {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "guidance belongs to another child",
        ));
    }
    insert(db, source, &origin)
}

pub(super) fn admit(
    database: &mut Database,
    parent: TurnId,
    request: &ToolConfirmationRequest,
    arguments: Value,
    selected: Option<Input>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Invocation, Fault> {
    let has_role = arguments.get("role").is_some();
    if arguments.get("session").is_some_and(Value::is_null) {
        return Err(sailry_protocol::Fault::invalid("invalid child session"));
    }
    let args: Arguments = serde_json::from_value(arguments)
        .map_err(|_| sailry_protocol::Fault::invalid("invalid delegation arguments"))?;
    if args
        .title
        .as_ref()
        .is_some_and(|title| title.trim().is_empty() || title.contains(['\n', '\r']))
    {
        return Err(sailry_protocol::Fault::invalid("invalid delegation title"));
    }
    let inherits_input = selected.is_some();
    let message = selected.unwrap_or_else(|| args.task.clone().into());
    queue::validate(&message)?;
    if super::plugins::operation(&database.connection, parent, &request.tool_name)?
        != Some(sailry_protocol::tool::Operation::DelegateAgent)
    {
        return Err(sailry_protocol::Fault::invalid("invalid delegation tool"));
    }
    if let Some(worktree) = args.worktree
        && !database.worktree_root(worktree)?.is_dir()
    {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "worktree directory is unavailable",
        ));
    }
    if let Some(id) = args.session {
        let session = super::super::commands::session(&database.connection, id)?;
        if !database.worktree_root(session.worktree)?.is_dir() {
            return Err(Fault::new(
                ErrorCode::NotFound,
                "worktree directory is unavailable",
            ));
        }
    }
    let tx = database.connection.transaction().map_err(storage_error)?;
    let run = calls::active(&tx, parent)?;
    let previous = super::super::commands::session(&tx, run.session)?;
    super::super::sessions::writable(&previous)?;
    let (entry, index) = calls::locate(&tx, parent, request)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_delegations WHERE parent_turn=?1 AND entry=?2 AND part=?3)",
        params![parent.to_string(), entry, index as i64], |row| row.get(0),
    ).map_err(storage_error)?;
    if exists {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "child call was already admitted; execution was not replayed",
        ));
    }
    let (mut session, role, created) = if args.session.is_some() {
        let (session, role) = reuse::restore(&tx, &run, &previous, &args, has_role)?;
        (session, role, false)
    } else {
        let (mut session, role) = fresh::capture(&tx, database.node, &run, &previous, &args)?;
        session.creation_order = super::super::sessions::insert(&tx, &session)?;
        super::super::media::freeze(&tx, session.id, Some(run.session))?;
        (session, role, true)
    };
    let worktree = session.worktree;
    if inherits_input && worktree != run.worktree {
        return Err(sailry_protocol::Fault::invalid(
            "selected input cannot be delegated to another worktree",
        ));
    }
    let references = if created {
        super::plugins::read(&tx, run.turn)?
    } else if let Some(binding) = &session.config.assistant {
        super::plugins::assistant(&tx, binding)?
    } else {
        super::plugins::capture(&tx)?
    };
    let plugins = super::plugins::packages(&tx, &references)?;
    if !created && let Some(role) = &role {
        let resources: Vec<_> = plugins
            .iter()
            .filter(|package| {
                crate::plugins::conversation::owns_resources(
                    session.config.assistant.as_ref(),
                    package,
                )
            })
            .cloned()
            .collect();
        for key in &role.skills {
            crate::plugins::resources::skill(&resources, key)?;
        }
    }
    let plugin_settings = super::super::plugins::settings::capture(&tx, &plugins);
    let (caller, request_id): (Vec<u8>, String) = tx
        .query_row(
            "SELECT caller,request FROM turns WHERE id=?1",
            [parent.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(storage_error)?;
    let caller = NodeId(
        caller
            .try_into()
            .map_err(|_| storage_error("invalid caller identity"))?,
    );
    let media = super::super::media::session_models(&tx, session.id)?;
    let origin = Delegation {
        session: run.session,
        turn: parent,
        entry,
        index,
        role: role.as_ref().map(|role| role.id),
    };
    if created {
        session.delegation = Some(Box::new(origin.clone()));
    }
    let turn = QueuedTurn {
        id: TurnId::new(),
        kind: RunKind::Task,
        session: session.id,
        request: request_id.parse().map_err(storage_error)?,
        revision: session.revision,
        config: session.config.clone(),
        roles: session.roles.clone(),
        plugins: references,
    };
    // The outer request owns durable admission; child rows only freeze scope and ancestry.
    tx.execute(
        "INSERT INTO turns(id,kind,session,revision,request,caller,plugins) VALUES(?1,'task',?2,?3,?4,?5,?6)",
        params![
            turn.id.to_string(),
            session.id.to_string(),
            turn.revision as i64,
            request_id,
            &caller.0[..],
            encode(&turn.plugins)?,
        ],
    )
    .map_err(storage_error)?;
    insert(&tx, turn.id, &origin)?;
    // Child work already has a supervisor slot; it must not wait behind its parent in the root queue.
    super::super::attachments::turns::bind(&tx, turn.id, &message.attachments)?;
    runs::record(&tx, &turn, Status::Running, false)?;
    runs::capture(&tx, &turn, false, &message)?;
    let provider: Option<Vec<u8>> = tx
        .query_row(
            "SELECT provider FROM agent_runs WHERE turn=?1",
            [turn.id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let provider: Option<Provider> = provider
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?;
    let media = super::super::media::for_turn(media, provider.as_ref(), &turn.config);
    let child_run = runs::get(&tx, turn.id)?;
    let summary = children::get(&tx, &child_run)?.expect("admitted child has an origin");
    let background = super::background::child_started(
        &tx,
        &run,
        &child_run,
        args.title.as_deref().unwrap_or(&args.task),
    )?;
    let mut envelopes = Vec::with_capacity(2);
    for event in created
        .then(|| Event::SessionChanged(Box::new(session)))
        .into_iter()
        .chain(std::iter::once(activity::queued(&tx, turn.clone())?))
    {
        tx.execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
            .map_err(storage_error)?;
        envelopes.push(EventEnvelope {
            node: database.node,
            cursor: tx.last_insert_rowid() as u64,
            event,
        });
    }
    tx.commit().map_err(storage_error)?;
    // Activate temporary output before returning the invocation to the runner.
    super::feed::run(database, child_run);
    super::background::publish(database, [background]);
    database.feeds.publish(
        database.node,
        summary.origin.session,
        Change::Child(summary),
    );
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(Invocation {
        hooks: super::super::hooks::frozen(&database.connection, turn.id)?,
        automatic: false,
        notice: false,
        notice_input: None,
        notices: Vec::new(),
        title: None,
        project: previous.project,
        connections: connections::catalog(&database.connection, turn.session)?,
        media,
        turn,
        caller,
        worktree,
        provider,
        message,
        child: Some(Child { role }),
        plugins,
        plugin_settings,
    })
}
