//! MCP authorization shares the existing login lifecycle and durable admission owner.
use super::*;
use crate::{
    plugins::{authorization::Flow, resources::Resources},
    store::{
        database::{encode, storage_error},
        plugins::authorization,
    },
};
use sailry_protocol::plugin::authorization::{Attempt, State, Update as LoginUpdate};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) struct Entry {
    updates: watch::Sender<LoginUpdate>,
    callback: Option<oneshot::Sender<Secret>>,
    stop: CancellationToken,
    finished: bool,
}

pub(in crate::store) enum Progress {
    Prompt {
        caller: NodeId,
        id: RequestId,
        url: String,
        expires_at_ms: u64,
    },
    Finished {
        caller: NodeId,
        id: RequestId,
        result: Result<(String, Secret), Fault>,
    },
}

impl Worker {
    pub(super) fn mcp_command(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        match &request.command {
            Command::BeginMcpLogin { .. } => self.begin_mcp(database, caller, request, events),
            Command::CompleteMcpLogin { attempt, callback } => {
                let result = self
                    .mcp
                    .get(&(caller, *attempt))
                    .ok_or_else(missing)
                    .and_then(|entry| {
                        if !matches!(entry.updates.borrow().state, State::Pending { .. })
                            || entry.callback.is_none()
                        {
                            return Err(Fault::new(
                                ErrorCode::Conflict,
                                "MCP authorization is not awaiting a callback",
                            ));
                        }
                        if callback.expose().len() > 16 * 1024
                            || callback.expose().chars().any(char::is_control)
                        {
                            return Err(Fault::new(
                                ErrorCode::InvalidRequest,
                                "invalid MCP authorization callback",
                            ));
                        }
                        Ok(Output::McpLogin(entry.updates.borrow().attempt.clone()))
                    });
                let result = database.finish_external(caller, request, result, events)?;
                if let Some(entry) = self.mcp.get_mut(&(caller, *attempt))
                    && let Some(sender) = entry.callback.take()
                {
                    entry.set(State::Exchanging);
                    let _ = sender.send(callback.clone());
                }
                Ok(result)
            }
            Command::CancelMcpLogin { attempt } => {
                let result = self
                    .mcp
                    .get(&(caller, *attempt))
                    .ok_or_else(missing)
                    .and_then(|entry| {
                        if !entry.updates.borrow().state.active() {
                            return Err(Fault::new(
                                ErrorCode::Conflict,
                                "MCP authorization has already finished",
                            ));
                        }
                        Ok(Output::McpLogin(entry.updates.borrow().attempt.clone()))
                    });
                let result = database.finish_external(caller, request, result, events)?;
                if let Some(entry) = self.mcp.get(&(caller, *attempt)) {
                    entry.stop.cancel();
                    entry.set(State::Cancelled);
                }
                Ok(result)
            }
            _ => unreachable!(),
        }
    }

    fn begin_mcp(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        let Command::BeginMcpLogin {
            package,
            server,
            redirect,
            client_id,
        } = &request.command
        else {
            unreachable!()
        };
        let client_id = client_id.as_deref();
        let server = server.as_str();
        let prepared = (|| {
            if self.closed.is_cancelled() {
                return Err(super::super::unavailable());
            }
            let info = authorization::current(&database.connection, package, server)?;
            validate_redirect(redirect)?;
            if client_id.is_some_and(|id| {
                id.is_empty() || id.len() > 2048 || id.chars().any(char::is_control)
            }) {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "invalid MCP OAuth client ID",
                ));
            }
            if self.mcp.values().filter(|entry| !entry.finished).count() >= 4
                || self.mcp.values().any(|entry| {
                    let update = entry.updates.borrow();
                    !entry.finished
                        && update.attempt.package.name == package.name
                        && update.attempt.server == server
                })
            {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "MCP authorization is already active or capacity is exhausted",
                ));
            }
            Ok(info)
        })();
        let info = match prepared {
            Ok(info) => info,
            Err(error) => return database.finish_external(caller, request, Err(error), events),
        };
        let attempt = Attempt {
            id: request.id,
            package: package.clone(),
            server: server.into(),
        };
        let result = database.finish_external(
            caller,
            request,
            Ok(Output::McpLogin(attempt.clone())),
            events,
        )?;
        while self.mcp.len() >= 32 {
            let Some(key) = self
                .mcp
                .iter()
                .find(|(_, entry)| entry.finished)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.mcp.remove(&key);
        }
        let (updates, _) = watch::channel(LoginUpdate {
            attempt: attempt.clone(),
            revision: 1,
            state: State::Starting,
        });
        let (callback, returned) = oneshot::channel();
        let stop = self.closed.child_token();
        self.mcp.insert(
            (caller, request.id),
            Entry {
                updates,
                callback: Some(callback),
                stop: stop.clone(),
                finished: false,
            },
        );
        self.pending += 1;
        let resources = Resources::new(
            self.plugins.clone(),
            vec![info],
            BTreeMap::new(),
            stop.clone(),
        );
        let (redirect, client_id, sender) = (
            redirect.to_owned(),
            client_id.map(str::to_owned),
            self.sender.clone(),
        );
        self.runtime.spawn(async move {
            let id = attempt.id;
            let operation = async {
                let expires_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_err(storage_error)?.as_millis() as u64 + 900_000;
                let endpoint = resources.mcp_endpoint(&attempt.package.name, &attempt.server).await?;
                let flow = Flow::begin(&endpoint, &redirect, client_id.as_deref()).await?;
                sender.send(Job::Login(Box::new(super::Progress::Mcp(Progress::Prompt { caller, id, url: flow.url().into(), expires_at_ms })))).await.map_err(|_| super::super::unavailable())?;
                let callback = returned.await.map_err(|_| super::super::unavailable())?;
                let credentials = flow.complete(&callback).await?;
                Ok((endpoint, credentials))
            };
            let result = tokio::select! {
                biased;
                _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "MCP authorization cancelled")),
                result = tokio::time::timeout(Duration::from_secs(900), AssertUnwindSafe(operation).catch_unwind()) => result.ok().and_then(Result::ok).unwrap_or_else(|| Err(super::super::unavailable())),
            };
            let _ = sender.send(Job::Login(Box::new(super::Progress::Mcp(Progress::Finished { caller, id, result })))).await;
        });
        Ok(result)
    }

    pub(super) fn mcp_progress(
        &mut self,
        database: &mut Database,
        progress: Progress,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        match progress {
            Progress::Prompt {
                caller,
                id,
                url,
                expires_at_ms,
            } => {
                if let Some(entry) = self.mcp.get(&(caller, id))
                    && entry.updates.borrow().state.active()
                    && !entry.stop.is_cancelled()
                {
                    entry.set(State::Pending { url, expires_at_ms });
                }
            }
            Progress::Finished { caller, id, result } => {
                let Some(entry) = self.mcp.get_mut(&(caller, id)) else {
                    return;
                };
                if entry.finished {
                    return;
                }
                entry.finished = true;
                self.pending -= 1;
                entry.callback.take();
                if !entry.updates.borrow().state.active() {
                    return;
                }
                if entry.stop.is_cancelled() {
                    entry.set(State::Cancelled);
                    return;
                }
                let attempt = entry.updates.borrow().attempt.clone();
                let result = result.and_then(|(endpoint, credentials)| {
                    save(database, &attempt, &endpoint, credentials, events)
                });
                entry.set(match result {
                    Ok(package) => State::Connected(package),
                    Err(error) => State::Failed(error),
                });
            }
        }
    }

    pub(in crate::store) fn subscribe_mcp(
        &self,
        caller: NodeId,
        id: RequestId,
    ) -> Result<Box<dyn Subscription>, Fault> {
        let entry = self.mcp.get(&(caller, id)).ok_or_else(missing)?;
        Ok(Box::new(Updates {
            receiver: entry.updates.subscribe(),
            initial: true,
            closed: self.closed.clone(),
        }))
    }
}

impl Entry {
    fn set(&self, state: State) {
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
                tokio::select! {
                    _ = self.closed.cancelled() => return Err(super::super::unavailable()),
                    result = self.receiver.changed() => result.map_err(|_| missing())?,
                }
            }
            self.initial = false;
            Ok(Update::McpLogin(self.receiver.borrow_and_update().clone()))
        })
    }
}

fn save(
    database: &mut Database,
    attempt: &Attempt,
    endpoint: &str,
    credentials: Secret,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<plugin::Reference, Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let info = authorization::save(
        &transaction,
        &attempt.package,
        &attempt.server,
        endpoint,
        credentials,
    )?;
    let package = info.summary.reference();
    let event = Event::PluginChanged(info.summary);
    transaction
        .execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
        .map_err(storage_error)?;
    let envelope = EventEnvelope {
        node: database.node,
        cursor: transaction.last_insert_rowid() as u64,
        event,
    };
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    Ok(package)
}

fn validate_redirect(value: &str) -> Result<(), Fault> {
    let invalid = || {
        Fault::new(
            ErrorCode::InvalidRequest,
            "invalid MCP authorization redirect URI",
        )
    };
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return Err(invalid());
    }
    let url = reqwest::Url::parse(value).map_err(|_| invalid())?;
    crate::plugins::mcp::validate_http(value, &BTreeMap::new()).map_err(|_| invalid())?;
    if url.query().is_some() {
        return Err(invalid());
    }
    Ok(())
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "MCP authorization expired or belongs to another controller",
    )
}
