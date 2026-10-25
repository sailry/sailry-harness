//! Child admission references a parent call and reuses the sole session history path.
use super::*;
use adk_core::ToolConfirmationRequest;
use serde::Deserialize;
use serde_json::Value;

mod claim;
pub(super) mod control;
pub(super) use claim::{claim, finish_queued};
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
    pub(crate) async fn claim_child(
        &self,
        turn: TurnId,
        token: sailry_link::CancellationToken,
    ) -> Result<Status, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::ClaimChild {
            turn,
            token,
            reply,
        })))
        .await
        .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }

    pub(crate) async fn finish_queued_child(
        &self,
        turn: TurnId,
        status: Status,
        error: Option<Fault>,
    ) -> Result<Status, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::FinishQueuedChild {
            turn,
            status,
            error,
            reply,
        })))
        .await
        .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }

    pub(crate) async fn delegate(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
        arguments: Value,
        message: Option<Input>,
        token: sailry_link::CancellationToken,
    ) -> Result<Invocation, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::Delegate {
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

pub(in crate::store) async fn read(
    db: &Connection,
    session: SessionId,
) -> Result<Option<Box<Delegation>>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db.prepare("SELECT d.body FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=?1 ORDER BY t.rowid LIMIT 1").await.map_err(storage_error)?;
        match statement.query_row([session.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(in crate::store) async fn read_only(
    db: &Connection,
    session: SessionId,
) -> Result<bool, Fault> {
    let turn: String = {
        let mut statement = db.prepare("SELECT t.id FROM turns t JOIN agent_delegations d ON d.turn=t.id WHERE t.session=?1 ORDER BY t.rowid LIMIT 1").await.map_err(storage_error)?;
        let row = statement
            .query_row([session.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    let turn = turn.parse().map_err(storage_error)?;
    Ok(super::permissions::config(db, turn).await?.mode == WorkMode::Plan)
}

pub(super) async fn origin(db: &Connection, turn: TurnId) -> Result<Option<Delegation>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_delegations WHERE turn=?1")
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
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

pub(in crate::store) async fn child(db: &Connection, turn: TurnId) -> Result<Option<Child>, Fault> {
    let Some(origin) = origin(db, turn).await? else {
        return Ok(None);
    };
    let run = runs::get(db, turn).await?;
    let founding = read(db, run.session)
        .await?
        .ok_or_else(|| storage_error("child session has no founding delegation"))?;
    if origin.session != founding.session || origin.role != founding.role {
        return Err(storage_error(
            "child ownership differs from its founding delegation",
        ));
    }
    let role = if let Some(id) = founding.role {
        let parent = runs::get(db, founding.turn).await?;
        let roster = super::super::sessions::roles::for_turn(db, parent.turn).await?;
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

async fn insert(db: &Connection, turn: TurnId, origin: &Delegation) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO agent_delegations(turn,parent_turn,entry,part,body) VALUES(?1,?2,?3,?4,?5)",
        params![
            turn.to_string(),
            origin.turn.to_string(),
            origin.entry.as_str(),
            origin.index as i64,
            encode(origin)?
        ],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn follow(
    db: &Connection,
    recipient: TurnId,
    source: TurnId,
) -> Result<(), Fault> {
    let origin = origin(db, recipient)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::WrongTarget, "guidance recipient is not a child"))?;
    if runs::get(db, recipient).await?.session != runs::get(db, source).await?.session {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "guidance belongs to another child",
        ));
    }
    insert(db, source, &origin).await
}

pub(super) async fn admit(
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
    let operation =
        super::plugins::operation(&database.connection, parent, &request.tool_name).await?;
    if operation != Some(sailry_protocol::tool::Operation::DelegateAgent)
        && !(operation == Some(sailry_protocol::tool::Operation::GuideAgent)
            && args.session.is_some())
    {
        return Err(sailry_protocol::Fault::invalid("invalid delegation tool"));
    }
    if let Some(worktree) = args.worktree
        && !database.worktree_root(worktree).await?.is_dir()
    {
        return Err(Fault::new(
            ErrorCode::NotFound,
            "worktree directory is unavailable",
        ));
    }
    if let Some(id) = args.session {
        let session = super::super::commands::session(&database.connection, id).await?;
        if !database.worktree_root(session.worktree).await?.is_dir() {
            return Err(Fault::new(
                ErrorCode::NotFound,
                "worktree directory is unavailable",
            ));
        }
    }
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result: Result<_, Fault> = async {
        let mut tx = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        tx.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let run = calls::active(&tx, parent).await?;
        let previous = super::super::commands::session(&tx, run.session).await?;
        super::super::sessions::writable(&previous)?;
        let (entry, index) = calls::locate(&tx, parent, request).await?;
        let exists: bool = {
            let mut statement = tx.prepare("SELECT EXISTS(SELECT 1 FROM agent_delegations WHERE parent_turn=?1 AND entry=?2 AND part=?3)").await.map_err(storage_error)?;
            let row = statement
                .query_row(params![parent.to_string(), entry.as_str(), index as i64])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        if exists {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "child call was already admitted; execution was not replayed",
            ));
        }
        let (mut session, role, created) = if args.session.is_some() {
            let (session, role) = reuse::restore(&tx, &run, &previous, &args, has_role).await?;
            (session, role, false)
        } else {
            let (mut session, role) =
                fresh::capture(&tx, database.node, &run, &previous, &args).await?;
            session.creation_order = super::super::sessions::insert(&tx, &session).await?;
            super::super::media::freeze(&tx, session.id, Some(run.session)).await?;
            (session, role, true)
        };
        let worktree = session.worktree;
        if inherits_input && worktree != run.worktree {
            return Err(sailry_protocol::Fault::invalid(
                "selected input cannot be delegated to another worktree",
            ));
        }
        let references = if created {
            super::plugins::read(&tx, run.turn).await?
        } else if let Some(binding) = &session.config.assistant {
            super::plugins::assistant(&tx, binding).await?
        } else {
            super::plugins::capture(&tx).await?
        };
        let plugins = super::plugins::packages(&tx, &references).await?;
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
        let plugin_settings = super::super::plugins::settings::capture(&tx, &plugins).await;
        let (caller, request_id): (Vec<u8>, String) = {
            let mut statement = tx
                .prepare("SELECT caller,request FROM turns WHERE id=?1")
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row([parent.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
            mapped.map_err(storage_error)?
        };
        let caller = NodeId(
            caller
                .try_into()
                .map_err(|_| storage_error("invalid caller identity"))?,
        );
        let media = super::super::media::session_models(&tx, session.id).await?;
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
            "INSERT INTO turns(id,kind,session,revision,request,caller,plugins,roles,rowid) VALUES(?1,'task',?2,?3,?4,?5,?6,?7,?8)",
            params![
                turn.id.to_string(),
                session.id.to_string(),
                turn.revision as i64,
                request_id,
                &caller.0[..],
                encode(&turn.plugins)?,
                encode(&turn.roles)?, crate::store::database::counters::next(&tx, crate::store::database::counters::Namespace::Turns).await? as i64],
        ).await
        .map_err(storage_error)?;
        super::super::plugins::connectors::capture(&tx, turn.id, &turn.plugins).await?;
        insert(&tx, turn.id, &origin).await?;
        // The supervisor owns the waiter before admission. Only the child claim may
        // start execution; root workers can be waiting for these children.
        super::super::attachments::turns::bind(&tx, turn.id, &message.attachments).await?;
        runs::record(&tx, &turn, Status::Queued, true).await?;
        runs::capture(&tx, &turn, false, &message).await?;
        let provider: Option<Vec<u8>> = {
            let mut statement = tx
                .prepare("SELECT provider FROM agent_runs WHERE turn=?1")
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row([turn.id.to_string()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        let provider: Option<Provider> = provider
            .map(|body| serde_json::from_slice(&body).map_err(storage_error))
            .transpose()?;
        if let Some(provider) = &provider {
            crate::store::providers::presets::verify(&tx, provider).await?;
        }
        let media = super::super::media::for_turn(media, provider.as_ref(), &turn.config);
        let child_run = runs::get(&tx, turn.id).await?;
        let summary = children::get(&tx, &child_run)
            .await?
            .expect("admitted child has an origin");
        let background = super::background::child_started(
            &tx,
            &run,
            &child_run,
            args.title.as_deref().unwrap_or(&args.task),
        )
        .await?;
        let queued = activity::queued(&tx, turn.clone()).await?;
        let changes = created
            .then(|| Event::SessionChanged(Box::new(session)))
            .into_iter()
            .chain(std::iter::once(queued))
            .collect();
        let envelopes = super::super::database::record(&tx, database.node, changes).await?;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(tx).await?;
        drop(write.take());
        // Activate temporary output before returning the invocation to the runner.
        super::feed::run(database, child_run).await;
        super::background::publish(database, [background]).await;
        database.feeds.publish(
            database.node,
            summary.origin.session,
            Change::Child(summary),
        );
        for envelope in envelopes {
            publication.send(events, envelope);
        }
        drop(publication);
        Ok(Invocation {
            instructions: super::super::captures::instructions(&database.connection, turn.id).await?,
            hooks: super::super::hooks::frozen(&database.connection, turn.id).await?,
            automatic: false,
            notice: false,
            notice_input: None,
            notices: Vec::new(),
            title: None,
            project: previous.project,
            connections: connections::catalog(&database.connection, turn.session).await?,
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
    .await;
    if let Some(write) = write {
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        drop(write);
    }
    result
}
