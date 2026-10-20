//! Connections share the existing durable login worker and caller-bound attempts.
use super::*;
use crate::store::database::storage_error;
use crate::store::plugins::connectors::{self as connections, Grant, Record};
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
    pub(super) fn inspect_connector(
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
            let _ = reply.send(database.finish_external(
                caller,
                &request,
                Err(Fault::new(ErrorCode::Busy, "connector workers are busy")),
                events,
            ));
            return;
        }
        let declaration = connections::declaration(&database.connection, package, connector);
        let declaration = match declaration {
            Ok((_, declaration)) if request.plugin.is_none() => declaration,
            Ok(_) => {
                let _ = reply.send(database.finish_external(
                    caller,
                    &request,
                    Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "local credential inspection requires a controller",
                    )),
                    events,
                ));
                return;
            }
            Err(error) => {
                let _ = reply.send(database.finish_external(caller, &request, Err(error), events));
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
    pub(super) fn connector_command(
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
                let prepared = (|| {
                    if self.closed.is_cancelled() {
                        return Err(super::super::unavailable());
                    }
                    let (mut record, declaration) = connections::create(
                        &database.connection,
                        database.profile.as_deref(),
                        &request.command,
                    )?;
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
                    let info =
                        connections::declaration(&database.connection, package, connector)?.0;
                    let settings =
                        crate::store::plugins::settings::resolve(&database.connection, &info)?;
                    Ok((record, declaration, info, settings))
                })();
                let (record, declaration, info, settings) = match prepared {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        return database.finish_external(caller, request, Err(error), events);
                    }
                };
                let attempt = Attempt {
                    id: request.id,
                    connection: record.id,
                    package: package.clone(),
                    revision: record.revision,
                };
                let output = database.finish_external_with(
                    caller,
                    request,
                    Ok(Output::ConnectorLogin(attempt.clone())),
                    events,
                    |db, _| {
                        connections::save(db, &record)?;
                        Ok(Some(Event::PluginChanged(info.summary.clone())))
                    },
                )?;
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
                    return database.finish_external(
                        caller,
                        request,
                        Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "connector attempt belongs to another surface",
                        )),
                        events,
                    );
                }
                if entry.finished
                    || entry.callback.is_none()
                    || !entry.updates.borrow().state.active()
                    || input.expose().len() > 16 * 1024
                    || input.expose().chars().any(char::is_control)
                {
                    return database.finish_external(
                        caller,
                        request,
                        Err(Fault::invalid("connector is not awaiting valid input")),
                        events,
                    );
                }
                let output = Output::ConnectorLogin(entry.updates.borrow().attempt.clone());
                let result = database.finish_external(caller, request, Ok(output), events)?;
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
                    return database.finish_external(
                        caller,
                        request,
                        Err(Fault::new(
                            ErrorCode::PermissionDenied,
                            "connector attempt belongs to another surface",
                        )),
                        events,
                    );
                }
                let output = Output::ConnectorLogin(entry.updates.borrow().attempt.clone());
                if entry.finished {
                    return database.finish_external(caller, request, Ok(output), events);
                }
                let baseline = entry.record.clone();
                let result = database.finish_external_with(
                    caller,
                    request,
                    Ok(output),
                    events,
                    |db, _| {
                        let mut record = match connections::read(db, baseline.id) {
                            Ok(record) => record,
                            Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
                            Err(error) => return Err(error),
                        };
                        if record.revision != baseline.revision {
                            return Ok(None);
                        }
                        record.state = connector::State::NotConnected;
                        connections::save(db, &record)?;
                        Ok(super::super::plugins::get(db, &record.plugin)?
                            .map(|info| Event::PluginChanged(info.summary)))
                    },
                )?;
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

    pub(super) fn probe_connector(
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
            let _ = reply.send(database.finish_external(
                caller,
                &request,
                Err(Fault::new(ErrorCode::Busy, "connector workers are busy")),
                events,
            ));
            return;
        }
        let prepared = (|| {
            let record = connections::read(&database.connection, connection)?;
            connections::current_target(
                &database.connection,
                database.profile.as_deref(),
                connection,
                record.revision,
            )
            .map(|target| (record, target))
        })();
        let (baseline, target) = match prepared {
            Ok(target) => target,
            Err(error) => {
                let _ = reply.send(database.finish_external(caller, &request, Err(error), events));
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
                if target.method == connector::Kind::LocalCli {
                    let identity = host
                        .connectors
                        .inspect_cli(&target.declaration, &ingress.environment, &stop)
                        .await?;
                    if target
                        .identity
                        .as_ref()
                        .is_none_or(|expected| expected.id != identity.id)
                    {
                        return Err(Fault::new(
                            ErrorCode::NotConfigured,
                            "local account changed; sign in again",
                        ));
                    }
                    target.identity = Some(identity);
                } else if matches!(
                    target.method,
                    connector::Kind::Application | connector::Kind::Oauth2
                ) {
                    host.connectors
                        .access(&target, &ingress, None, &stop)
                        .await?;
                    target.grant = ingress
                        .connector_target(target.id, target.revision)
                        .await?
                        .grant;
                } else if target.method == connector::Kind::McpOauth {
                    let connector::Auth::McpOauth { server, .. } = target
                        .declaration
                        .auth
                        .iter()
                        .find(|auth| auth.kind() == target.method)
                        .ok_or_else(missing)?
                    else {
                        return Err(missing());
                    };
                    let endpoint = target.mcp_endpoint.clone().ok_or_else(missing)?;
                    let registered = crate::plugins::authorization::Registered::connector(&target)?;
                    let client = host
                        .authorizations
                        .client(
                            &ingress,
                            crate::plugins::authorization::Target {
                                plugin: target.package.name.clone(),
                                server: server.clone(),
                                endpoint,
                                id: target.id,
                                revision: target.revision,
                            },
                            registered,
                        )
                        .await?;
                    client.get_access_token().await.map_err(|_| missing())?;
                    target.grant = ingress
                        .connector_target(target.id, target.revision)
                        .await?
                        .grant;
                }
                host.connectors.probe(&mut target, &stop).await?;
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

    pub(super) fn connector_progress(
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
                let result = commit(database, &entry.record, result, true, events);
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
                let result =
                    commit(database, &baseline, result, false, events).map(Output::Connection);
                let _ = reply.send(database.finish_external(caller, &request, result, events));
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

fn commit(
    database: &mut Database,
    baseline: &Record,
    result: Result<Box<crate::plugins::connectors::Target>, Fault>,
    replace: bool,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<connector::Connection, Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let mut record = connections::read(&transaction, baseline.id)?;
    if record.revision != baseline.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "connection changed",
        ));
    }
    let info = connections::info(&transaction, &record.plugin)?;
    let (_, declaration) =
        connections::declaration(&transaction, &info.summary.reference(), &record.connector)?;
    if connections::bound(
        &transaction,
        database.profile.as_deref(),
        &info,
        &declaration,
        record.method,
    )? != record.bound
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "connector authorization changed",
        ));
    }
    let outcome = match result {
        Ok(target) => {
            let target = *target;
            record.identity = target.identity;
            if replace {
                record.fields = target.fields;
                record.secrets = target.secrets;
                record.grant_revision = record
                    .grant_revision
                    .checked_add(1)
                    .filter(|revision| *revision <= i64::MAX as u64)
                    .ok_or_else(missing)?;
                record.grant = target.grant.map(|grant| Grant {
                    access: grant.access,
                    refresh: grant.refresh,
                    expires_at_ms: grant.expires_at_ms,
                    extra: grant.extra,
                });
            }
            record.state = connector::State::Connected;
            Ok(())
        }
        Err(error) => {
            record.state = if record.method == connector::Kind::LocalCli
                || error.code == ErrorCode::Cancelled
            {
                connector::State::NotConnected
            } else if error.code == ErrorCode::Expired {
                connector::State::Expired
            } else {
                connector::State::Failed
            };
            Err(error)
        }
    };
    record.checked_at_ms = Some(crate::plugins::connectors::now());
    connections::save(&transaction, &record)?;
    let projected = connections::project(&transaction, database.profile.as_deref(), &record)?;
    let envelopes = super::super::database::record(
        &transaction,
        database.node,
        vec![Event::PluginChanged(info.summary)],
    )?;
    transaction.commit().map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    outcome.map(|_| projected)
}
fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "connector authorization is unavailable",
    )
}
