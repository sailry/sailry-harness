//! Child admission references a parent call and reuses the sole session history path.
use super::*;
use adk_core::ToolConfirmationRequest;
use serde::Deserialize;
use serde_json::Value;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod worktree_tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    role: Option<String>,
    title: Option<String>,
    task: String,
    worktree: Option<WorktreeId>,
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
            "SELECT body FROM session_delegations WHERE session=?1",
            [session.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(super) fn admit(
    database: &mut Database,
    parent: TurnId,
    request: &ToolConfirmationRequest,
    arguments: Value,
    selected: Option<Input>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Invocation, Fault> {
    let args: Arguments =
        serde_json::from_value(arguments).map_err(|_| invalid("invalid delegation arguments"))?;
    if args.title.as_ref().is_some_and(|title| {
        title.trim().is_empty() || title.chars().count() > 80 || title.contains(['\n', '\r'])
    }) {
        return Err(invalid("invalid delegation title"));
    }
    let inherits_input = selected.is_some();
    let message = selected.unwrap_or_else(|| args.task.into());
    queue::validate(&message)?;
    if super::plugins::operation(&database.connection, parent, &request.tool_name)?
        != Some(sailry_protocol::tool::Operation::DelegateAgent)
    {
        return Err(invalid("invalid delegation tool"));
    }
    if let Some(worktree) = args.worktree
        && !database.worktree_root(worktree)?.is_dir()
    {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "worktree directory is unavailable",
        ));
    }
    let tx = database.connection.transaction().map_err(storage_error)?;
    let run = calls::active(&tx, parent)?;
    let previous = super::super::commands::session(&tx, run.session)?;
    super::super::sessions::writable(&previous)?;
    if previous.delegation.is_some() {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "child sessions cannot delegate",
        ));
    }
    let (entry, index) = calls::locate(&tx, parent, request)?;
    let worktree = match args.worktree {
        Some(worktree) => super::super::worktrees::select(
            &tx,
            previous.project.ok_or_else(|| {
                invalid("connection conversations cannot select a project worktree")
            })?,
            Some(worktree),
        )?,
        None => run.worktree,
    };
    if inherits_input && worktree != run.worktree {
        return Err(invalid(
            "selected input cannot be delegated to another worktree",
        ));
    }
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_delegations WHERE parent_turn=?1 AND entry=?2 AND part=?3)",
        params![parent.to_string(), entry, index as i64], |row| row.get(0),
    ).map_err(storage_error)?;
    if exists {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "child call was already admitted; execution was not replayed",
        ));
    }
    let roster = super::super::sessions::roles::read(&tx, run.session, run.revision)?;
    let role = match args.role.as_deref() {
        Some(key) => Some(
            roster
                .profiles
                .iter()
                .find(|role| role.key == key)
                .ok_or_else(|| invalid("role is not in the parent turn snapshot"))?
                .clone(),
        ),
        None if roster.profiles.is_empty() => None,
        None => return Err(invalid("select a role from the parent turn snapshot")),
    };
    let references = super::plugins::read(&tx, run.turn)?;
    let plugins = super::plugins::packages(&tx, &references)?;
    let plugin_settings = super::super::plugins::settings::capture(&tx, &plugins);
    let (config, provider, caller, request_id): (Vec<u8>, Option<Vec<u8>>, Vec<u8>, String) = tx.query_row(
        "SELECT r.config,a.provider,t.caller,t.request FROM turns t JOIN agent_runs a ON a.turn=t.id JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1",
        [parent.to_string()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).map_err(storage_error)?;
    let caller = NodeId(
        caller
            .try_into()
            .map_err(|_| storage_error("invalid caller identity"))?,
    );
    let mut config: SessionConfig = serde_json::from_slice(&config).map_err(storage_error)?;
    if let Some(role) = &role {
        let resources: Vec<_> = plugins
            .iter()
            .filter(|package| {
                crate::plugins::conversation::owns_resources(config.assistant.as_ref(), package)
            })
            .cloned()
            .collect();
        for key in &role.skills {
            crate::plugins::resources::skill(&resources, key)?;
        }
    }
    let mut provider: Option<Provider> = provider
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?;
    if let Some(model) = role.as_ref().and_then(|role| role.model.as_ref()) {
        let selected = roster
            .providers
            .iter()
            .find(|provider| provider.id == model.provider)
            .ok_or_else(|| invalid("frozen role provider is missing"))?
            .clone();
        let metadata = selected
            .models
            .iter()
            .find(|entry| entry.id == model.model)
            .ok_or_else(|| invalid("frozen role model is missing"))?;
        config.provider = selected.id;
        config.credential = selected.credential.clone();
        config.model = model.model.clone();
        config.effort = model.effort.unwrap_or(metadata.default_effort);
        provider = Some(selected);
    }
    if let Some(provider) = &mut provider {
        crate::providers::login::capture(provider)?;
    }
    super::super::commands::validate_config(&tx, database.node, &config)?;
    let session = Session {
        id: SessionId::new(),
        archived: false,
        activity: Default::default(),
        project: previous.project,
        worktree,
        revision: 1,
        config,
        roles: role::Snapshot::default(),
        fork: None,
        profile: provider.clone().map(|provider| SessionProfile {
            source: database.node,
            provider,
        }),
        delegation: Some(Box::new(Delegation {
            session: run.session,
            turn: parent,
            entry: entry.clone(),
            index,
            role: role.as_ref().map(|role| role.id),
        })),
    };
    super::super::sessions::insert(&tx, &session)?;
    super::super::media::freeze(&tx, session.id, Some(run.session))?;
    let media = super::super::media::session_models(&tx, session.id)?;
    tx.execute("INSERT INTO session_delegations(session,parent_turn,entry,part,body) VALUES(?1,?2,?3,?4,?5)",
        params![session.id.to_string(), parent.to_string(), entry, index as i64, encode(session.delegation.as_ref().unwrap())?],
    ).map_err(storage_error)?;
    let turn = QueuedTurn {
        id: TurnId::new(),
        kind: RunKind::Task,
        session: session.id,
        request: request_id.parse().map_err(storage_error)?,
        revision: 1,
        config: session.config.clone(),
        roles: session.roles.clone(),
        plugins: references,
    };
    // The outer request owns durable admission; child rows only freeze scope and ancestry.
    tx.execute(
        "INSERT INTO turns(id,kind,session,revision,request,caller,plugins) VALUES(?1,'task',?2,1,?3,?4,?5)",
        params![
            turn.id.to_string(),
            session.id.to_string(),
            request_id,
            &caller.0[..],
            encode(&turn.plugins)?,
        ],
    )
    .map_err(storage_error)?;
    // Child work already has a supervisor slot; it must not wait behind its parent in the root queue.
    super::super::attachments::turns::bind(&tx, turn.id, &message.attachments)?;
    runs::record(&tx, &turn, Status::Running, false)?;
    let child_run = runs::get(&tx, turn.id)?;
    let summary = children::get(&tx, &child_run)?.expect("admitted child has an origin");
    let mut envelopes = Vec::with_capacity(2);
    for event in [
        Event::SessionChanged(Box::new(session)),
        activity::queued(&tx, turn.clone())?,
    ] {
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
    database
        .feeds
        .publish(database.node, turn.session, Change::Run(child_run));
    database.feeds.publish(
        database.node,
        summary.origin.session,
        Change::Child(summary),
    );
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(Invocation {
        automatic: false,
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
