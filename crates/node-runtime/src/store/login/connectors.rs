//! Connections share the existing durable login worker and caller-bound attempts.
use super::*;
use crate::store::database::storage_error;
use crate::store::plugins::connectors::{self as connections, Record};
use sailry_protocol::plugin::connector::{self as connector, Attempt, LoginState, LoginUpdate};

pub(super) struct Entry {
    updates: watch::Sender<LoginUpdate>,
    callback: Option<oneshot::Sender<Secret>>,
    stop: CancellationToken,
    record: Record,
    context: Option<sailry_protocol::plugin::Context>,
    finished: bool,
}

pub(in crate::store) enum Progress {
    Inspected {
        reply: oneshot::Sender<Response>,
        result: Result<connector::Identity, Fault>,
    },
    Prompt {
        caller: NodeId,
        id: RequestId,
        state: LoginState,
    },
    Finished {
        caller: NodeId,
        id: RequestId,
        result: Result<Box<crate::plugins::connectors::Target>, Fault>,
    },
    Probed {
        baseline: Box<Record>,
        caller: NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<Response>,
        result: Result<Box<crate::plugins::connectors::Target>, Fault>,
    },
}

impl Worker {
    pub(super) async fn inspect_connector(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: Request,
        reply: oneshot::Sender<Response>,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        let Command::InspectConnectorCli {
            ref package,
            ref connector,
        } = request.command
        else {
            unreachable!()
        };
        if self.pending >= 16 || self.closed.is_cancelled() {
            let _ = reply.send(
                database
                    .finish_external(
                        caller,
                        &request,
                        Err(Fault::new(ErrorCode::Busy, "connector workers are busy")),
                        events,
                    )
                    .await,
            );
            return;
        }
        let declaration = connections::declaration(&database.connection, package, connector).await;
        let declaration = match declaration {
            Ok((_, declaration)) if request.plugin.is_none() => declaration,
            Ok(_) => {
                let _ = reply.send(
                    database
                        .finish_external(
                            caller,
                            &request,
                            Err(Fault::new(
                                ErrorCode::PermissionDenied,
                                "local credential inspection requires a controller",
                            )),
                            events,
                        )
                        .await,
                );
                return;
            }
            Err(error) => {
                let _ = reply.send(
                    database
                        .finish_external(caller, &request, Err(error), events)
                        .await,
                );
                return;
            }
        };
        self.pending += 1;
        let host = self.plugins.clone();
        let environment = self.environment.clone();
        let stop = self.closed.child_token();
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let result = host
                .connectors
                .inspect_cli(&declaration, &environment, &stop)
                .await;
            let _ = sender
                .send(Job::Login(Box::new(super::Progress::Connector(Box::new(
                    Progress::Inspected { reply, result },
                )))))
                .await;
        });
    }
    pub(super) async fn connector_command(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        match &request.command {
            Command::StartConnectorLogin {
                package,
                connector,
                connection,
                expected_revision: _,
                method,
                input,
            } => {
                let prepared = (async || {
                    if self.closed.is_cancelled() {
                        return Err(super::super::unavailable());
                    }
                    let (mut record, declaration) = connections::create(
                        &database.connection,
                        database.profile.as_deref(),
                        &request.command,
                    )
                    .await?;
                    let previous = if connection.is_some() {
                        Some(connections::read(&database.connection, record.id).await?)
                    } else {
                        None
                    };
                    if self
                        .connectors
                        .values()
                        .filter(|entry| !entry.finished)
                        .count()
                        >= 4
                        || self.connectors.values().any(|entry| {
                            !entry.finished
                                && entry.record.plugin == record.plugin
                                && entry.record.connector == record.connector
                        })
                    {
                        return Err(Fault::new(
                            ErrorCode::Busy,
                            "connector authorization is already active",
                        ));
                    }
                    if connection.is_some() {
                        record.revision = record
                            .revision
                            .checked_add(1)
                            .filter(|revision| *revision <= i64::MAX as u64)
                            .ok_or_else(missing)?;
                    }
                    record.state = connector::State::Connecting;
                    if *method == connector::Kind::LocalCli {
                        record.secrets.clear();
                        record.grant = None;
                    }
                    connections::login::begin(&mut record, request.id, previous);
                    let info = connections::declaration(&database.connection, package, connector)
                        .await?
                        .0;
                    let settings =
                        crate::store::plugins::settings::resolve(&database.connection, &info)
                            .await?;
                    Ok((record, declaration, info, settings))
                })()
                .await;
                let (record, declaration, info, settings) = match prepared {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        return database
                            .finish_external(caller, request, Err(error), events)
                            .await;
                    }
                };
                let attempt = Attempt {
                    id: request.id,
                    connection: record.id,
                    package: package.clone(),
                    revision: record.revision,
                };
                let output = database
                    .finish_external_with(
                        caller,
                        request,
                        Ok(Output::ConnectorLogin(attempt.clone())),
                        events,
                        async |db, _| {
                            connections::save(db, &record).await?;
                            Ok(Some(Event::PluginChanged(info.summary.clone())))
                        },
                    )
                    .await?;
                self.connectors
                    .retain(|_, entry| !entry.finished || entry.updates.receiver_count() > 0);
                let (updates, _) = watch::channel(LoginUpdate {
                    attempt: attempt.clone(),
                    revision: 1,
                    state: LoginState::Starting,
                });
                let (callback, returned) = oneshot::channel();
                let stop = self.closed.child_token();
                self.connectors.insert(
                    (caller, request.id),
                    Entry {
                        updates,
                        callback: Some(callback),
                        stop: stop.clone(),
                        record: record.clone(),
                        context: request.plugin.clone(),
                        finished: false,
                    },
                );
                self.pending += 1;
                let host = self.plugins.clone();
                let manual = info.mcp_source.is_some();
                let resources = crate::plugins::resources::Resources::new(
                    host.clone(),
                    vec![info],
                    [(package.name.clone(), Ok(settings))].into(),
                    stop.clone(),
                );
                let mut target = connections::target(&record, declaration, package.clone());
                target.manual = manual;
                target.identity = None;
                let service = self.service.clone();
                let environment = self.environment.clone();
                let sender = self.sender.clone();
                let input = input.clone();
                self.runtime.spawn(async move{
                    let id=attempt.id;
                    let operation=async{
                        if let Some(connector::Auth::McpOauth{server,..})=target.declaration.auth.iter().find(|auth|auth.kind()==target.method){target.mcp_endpoint=Some(resources.mcp_endpoint(&target.package.name,server).await?);}
                        host.connectors.authorize(crate::plugins::connectors::Login{target,input,host:host.clone(),service,environment,callback:Some(returned),stop:stop.clone()},|state|{
                            let state=match state{
                                login::State::Pending{verification_url,user_code,expires_at_ms}=>LoginState::Device{code:user_code,url:verification_url,expires_at_ms},
                                login::State::Browser{authorization_url,expires_at_ms}=>LoginState::Browser{url:authorization_url,expires_at_ms},
                                _=>LoginState::Exchanging,
                            };
                            let sender=sender.clone();async move{sender.send(Job::Login(Box::new(super::Progress::Connector(Box::new(Progress::Prompt{caller,id,state}))))).await.map_err(|_|super::super::unavailable())}
                        }).await.map(Box::new)
                    };
                    let result=tokio::select!{biased;_=stop.cancelled()=>Err(Fault::new(ErrorCode::Cancelled,"connector authorization cancelled")),result=tokio::time::timeout(std::time::Duration::from_secs(900),AssertUnwindSafe(operation).catch_unwind())=>result.ok().and_then(Result::ok).unwrap_or_else(||Err(super::super::unavailable()))};
                    let _=sender.send(Job::Login(Box::new(super::Progress::Connector(Box::new(Progress::Finished{caller,id,result}))))).await;
                });
                Ok(output)
            }
            Command::SubmitConnectorInput { attempt, input } => {
                let entry = self
                    .connectors
                    .get(&(caller, *attempt))
                    .ok_or_else(missing)?;
                if request.plugin.is_some() && entry.context.as_ref() != request.plugin.as_ref() {
                    return database
                        .finish_external(
                            caller,
                            request,
                            Err(Fault::new(
                                ErrorCode::PermissionDenied,
                                "connector attempt belongs to another surface",
                            )),
                            events,
                        )
                        .await;
                }
                if entry.finished
                    || entry.callback.is_none()
                    || !entry.updates.borrow().state.active()
                    || input.expose().len() > 16 * 1024
                    || input.expose().chars().any(char::is_control)
                {
                    return database
                        .finish_external(
                            caller,
                            request,
                            Err(Fault::invalid("connector is not awaiting valid input")),
                            events,
                        )
                        .await;
                }
                let output = Output::ConnectorLogin(entry.updates.borrow().attempt.clone());
                let result = database
                    .finish_external(caller, request, Ok(output), events)
                    .await?;
                if let Some(entry) = self.connectors.get_mut(&(caller, *attempt))
                    && let Some(callback) = entry.callback.take()
                {
                    entry.set(LoginState::Exchanging);
                    let _ = callback.send(input.clone());
                }
                Ok(result)
            }
            Command::CancelConnectorLogin { attempt } => {
                let entry = self
                    .connectors
                    .get(&(caller, *attempt))
                    .ok_or_else(missing)?;
                if request.plugin.is_some() && entry.context.as_ref() != request.plugin.as_ref() {
                    return database
                        .finish_external(
                            caller,
                            request,
                            Err(Fault::new(
                                ErrorCode::PermissionDenied,
                                "connector attempt belongs to another surface",
                            )),
                            events,
                        )
                        .await;
                }
                let output = Output::ConnectorLogin(entry.updates.borrow().attempt.clone());
                if entry.finished {
                    return database
                        .finish_external(caller, request, Ok(output), events)
                        .await;
                }
                let baseline = entry.record.clone();
                let result = database
                    .finish_external_with(caller, request, Ok(output), events, async |db, _| {
                        let mut record = match connections::read(db, baseline.id).await {
                            Ok(record) => record,
                            Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
                            Err(error) => return Err(error),
                        };
                        if !connections::login::finish(
                            &mut record,
                            *attempt,
                            connections::login::Outcome::Cancelled,
                        )? {
                            return Ok(None);
                        }
                        connections::save(db, &record).await?;
                        Ok(super::super::plugins::get(db, &record.plugin)
                            .await?
                            .map(|info| Event::PluginChanged(info.summary)))
                    })
                    .await?;
                if let Some(entry) = self.connectors.get_mut(&(caller, *attempt)) {
                    entry.finished = true;
                    entry.callback.take();
                    entry.stop.cancel();
                    entry.set(LoginState::Cancelled);
                    self.pending -= 1;
                }
                Ok(result)
            }
            _ => Err(Fault::invalid("connector login command expected")),
        }
    }

    pub(super) async fn probe_connector(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: Request,
        reply: oneshot::Sender<Response>,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        let Command::ProbeConnection { connection } = request.command else {
            unreachable!()
        };
        if self.pending >= 16 || self.closed.is_cancelled() {
            let _ = reply.send(
                database
                    .finish_external(
                        caller,
                        &request,
                        Err(Fault::new(ErrorCode::Busy, "connector workers are busy")),
                        events,
                    )
                    .await,
            );
            return;
        }
        let prepared = (async || {
            let record = connections::read(&database.connection, connection).await?;
            // A pending sign-in owns the row until it finishes.
            if record.login.is_some() {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "connector authorization is already active",
                ));
            }
            connections::current_target(
                &database.connection,
                database.profile.as_deref(),
                connection,
                record.auth_revision,
                true,
            )
            .await
            .map(|target| (record, target))
        })()
        .await;
        let (baseline, target) = match prepared {
            Ok(target) => target,
            Err(error) => {
                let _ = reply.send(
                    database
                        .finish_external(caller, &request, Err(error), events)
                        .await,
                );
                return;
            }
        };
        self.pending += 1;
        let host = self.plugins.clone();
        let ingress = self.ingress.clone();
        let sender = self.sender.clone();
        let stop = self.closed.child_token();
        self.runtime.spawn(async move {
            let mut target = target;
            let result = async {
                host.connectors.check(&mut target, &ingress, &stop).await?;
                Ok(Box::new(target))
            }
            .await;
            let _ = sender
                .send(Job::Login(Box::new(super::Progress::Connector(Box::new(
                    Progress::Probed {
                        baseline: Box::new(baseline),
                        caller,
                        request: Box::new(request),
                        reply,
                        result,
                    },
                )))))
                .await;
        });
    }

    pub(super) async fn connector_progress(
        &mut self,
        database: &mut Database,
        progress: Progress,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        match progress {
            Progress::Inspected { reply, result } => {
                self.pending -= 1;
                let _ = reply.send(result.map(Output::ConnectorIdentity));
            }
            Progress::Prompt { caller, id, state } => {
                if let Some(entry) = self.connectors.get(&(caller, id))
                    && !entry.finished
                    && !entry.stop.is_cancelled()
                {
                    entry.set(state);
                }
            }
            Progress::Finished { caller, id, result } => {
                let Some(entry) = self.connectors.get_mut(&(caller, id)) else {
                    return;
                };
                if entry.finished {
                    return;
                }
                entry.finished = true;
                entry.callback.take();
                self.pending -= 1;
                let result = commit(database, &entry.record, result, true, events).await;
                entry.set(match result {
                    Ok(connection) => LoginState::Connected(connection),
                    Err(error) if error.code == ErrorCode::Cancelled => LoginState::Cancelled,
                    Err(error) => LoginState::Failed(error),
                });
            }
            Progress::Probed {
                baseline,
                caller,
                request,
                reply,
                result,
            } => {
                self.pending -= 1;
                let result = commit(database, &baseline, result, false, events)
                    .await
                    .map(Output::Connection);
                let _ = reply.send(
                    database
                        .finish_external(caller, &request, result, events)
                        .await,
                );
            }
        }
    }

    pub(in crate::store) fn subscribe_connector(
        &self,
        caller: NodeId,
        id: RequestId,
    ) -> Result<Box<dyn Subscription>, Fault> {
        let entry = self.connectors.get(&(caller, id)).ok_or_else(missing)?;
        Ok(Box::new(Updates {
            receiver: entry.updates.subscribe(),
            initial: true,
            closed: self.closed.clone(),
        }))
    }
}

impl Entry {
    fn set(&self, state: LoginState) {
        self.updates.send_modify(|update| {
            update.revision += 1;
            update.state = state;
        });
    }
}
struct Updates {
    receiver: watch::Receiver<LoginUpdate>,
    initial: bool,
    closed: CancellationToken,
}
impl Subscription for Updates {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if !self.initial {
                tokio::select! {biased;_=self.closed.cancelled()=>return Err(super::super::unavailable()),result=self.receiver.changed()=>{result.map_err(|_|missing())?;}}
            }
            self.initial = false;
            Ok(Update::ConnectorLogin(
                self.receiver.borrow_and_update().clone(),
            ))
        })
    }
}

async fn commit(
    database: &mut Database,
    baseline: &Record,
    result: Result<Box<crate::plugins::connectors::Target>, Fault>,
    replace: bool,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<connector::Connection, Fault> {
    if !replace && let Err(error) = &result {
        return Err(error.clone());
    }
    let transaction = database
        .connection
        .unchecked_transaction()
        .await
        .map_err(storage_error)?;
    let mut record = connections::read(&transaction, baseline.id).await?;
    let checked = (async || {
        if record.revision != baseline.revision
            || record.auth_revision != baseline.auth_revision
            || record.login.as_ref().map(|login| login.attempt)
                != baseline.login.as_ref().map(|login| login.attempt)
        {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "connection changed",
            ));
        }
        let info = connections::info(&transaction, &record.plugin).await?;
        let (_, declaration) =
            connections::declaration(&transaction, &info.summary.reference(), &record.connector)
                .await?;
        let current = connections::bound(
            &transaction,
            database.profile.as_deref(),
            &info,
            &declaration,
            record.method,
        )
        .await?;
        if !connections::consent::same(&record.plugin, record.bound.as_ref(), current.as_ref()) {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "connector authorization changed",
            ));
        }
        Ok(())
    })()
    .await;
    // Authorization failures may mark a first connection Failed; conflicts never do.
    let (outcome, failed) = match (checked, result) {
        (Err(error), _) => (Err(error), false),
        (Ok(()), Err(error)) => {
            let failed = error.code != ErrorCode::Cancelled;
            (Err(error), failed)
        }
        (Ok(()), Ok(target)) => {
            let outcome = if replace {
                baseline
                    .login
                    .as_ref()
                    .ok_or_else(missing)
                    .and_then(|login| {
                        connections::login::finish(
                            &mut record,
                            login.attempt,
                            connections::login::Outcome::Connected(target),
                        )
                        .and_then(|finished| finished.then_some(()).ok_or_else(missing))
                    })
            } else {
                apply(&mut record, *target)
            };
            (outcome, false)
        }
    };
    let persist = async |transaction: turso::transaction::Transaction<'_>,
                         record: &Record|
           -> Result<connector::Connection, Fault> {
        connections::save(&transaction, record).await?;
        let projected =
            connections::project(&transaction, database.profile.as_deref(), record).await?;
        let changed = super::super::plugins::get(&transaction, &record.plugin)
            .await?
            .map(|info| Event::PluginChanged(info.summary))
            .into_iter()
            .collect();
        let envelopes =
            super::super::database::record(&transaction, database.node, changed).await?;
        transaction.commit().await.map_err(storage_error)?;
        for envelope in envelopes {
            let _ = events.send(envelope);
        }
        Ok(projected)
    };
    match outcome {
        Ok(()) => persist(transaction, &record).await,
        Err(error) => {
            if replace
                && let Some(login) = &baseline.login
                && connections::login::finish(
                    &mut record,
                    login.attempt,
                    if failed {
                        connections::login::Outcome::Failed
                    } else {
                        connections::login::Outcome::Cancelled
                    },
                )?
            {
                persist(transaction, &record).await?;
            }
            Err(error)
        }
    }
}

fn apply(record: &mut Record, target: crate::plugins::connectors::Target) -> Result<(), Fault> {
    if record.grant_revision != target.grant_revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "connector grant changed",
        ));
    }
    record.identity = target.identity;
    record.state = connector::State::Connected;
    record.checked_at_ms = Some(crate::plugins::connectors::now());
    Ok(())
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "connector authorization is unavailable",
    )
}
