//! Fixed-provider account login shares durable admission and authenticated observation.
use super::*;
use crate::{
    plugins::oauth::{Flow, Grant},
    store::{
        database::{encode, storage_error},
        plugins::oauth,
    },
};
use sailry_protocol::plugin::oauth::{Attempt, State, Update as LoginUpdate};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[path = "plugin/registration.rs"]
mod registration;

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
        result: Result<Authorized, Fault>,
    },
}

pub(in crate::store) enum Authorized {
    Account(Grant),
    Application(
        crate::plugins::registration::Credentials,
        crate::store::plugins::registration::Baseline,
    ),
}

impl Worker {
    pub(super) fn plugin_command(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        match &request.command {
            Command::BeginPluginLogin { .. } => {
                self.begin_plugin(database, caller, request, events)
            }
            Command::BeginPluginRegistration { .. } => {
                self.register_plugin(database, caller, request, events)
            }
            Command::CompletePluginLogin { attempt, callback } => {
                let result = self
                    .plugin
                    .get(&(caller, *attempt))
                    .ok_or_else(missing)
                    .and_then(|entry| {
                        if !matches!(entry.updates.borrow().state, State::Pending { .. })
                            || entry.callback.is_none()
                        {
                            return Err(Fault::new(
                                ErrorCode::Conflict,
                                "plugin account authorization is not awaiting a callback",
                            ));
                        }
                        if callback.expose().len() > 16 * 1024
                            || callback.expose().chars().any(char::is_control)
                        {
                            return Err(Fault::invalid(
                                "invalid plugin account authorization callback",
                            ));
                        }
                        Ok(Output::PluginLogin(entry.updates.borrow().attempt.clone()))
                    });
                let result = database.finish_external(caller, request, result, events)?;
                if let Some(entry) = self.plugin.get_mut(&(caller, *attempt))
                    && let Some(sender) = entry.callback.take()
                {
                    entry.set(State::Exchanging);
                    let _ = sender.send(callback.clone());
                }
                Ok(result)
            }
            Command::CancelPluginLogin { attempt } => {
                let result = self
                    .plugin
                    .get(&(caller, *attempt))
                    .ok_or_else(missing)
                    .and_then(|entry| {
                        if request.plugin.as_ref().is_some_and(|context| {
                            context.package != entry.updates.borrow().attempt.package
                        }) {
                            return Err(Fault::new(
                                ErrorCode::PermissionDenied,
                                "application registration attempt belongs to another package",
                            ));
                        }
                        if !entry.updates.borrow().state.active() {
                            return Err(Fault::new(
                                ErrorCode::Conflict,
                                "plugin account authorization has already finished",
                            ));
                        }
                        Ok(Output::PluginLogin(entry.updates.borrow().attempt.clone()))
                    });
                let result = database.finish_external(caller, request, result, events)?;
                if let Some(entry) = self.plugin.get(&(caller, *attempt)) {
                    entry.stop.cancel();
                    entry.set(State::Cancelled);
                }
                Ok(result)
            }
            _ => unreachable!(),
        }
    }

    fn begin_plugin(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        let Command::BeginPluginLogin {
            package,
            redirect,
            configuration,
        } = &request.command
        else {
            unreachable!()
        };
        let prepared = (|| {
            if self.closed.is_cancelled() {
                return Err(super::super::unavailable());
            }
            let (_, declaration) = oauth::current(&database.connection, package)?;
            let flow = Flow::begin(declaration, configuration.clone(), redirect)?;
            if self.plugin.values().filter(|entry| !entry.finished).count() >= 4
                || self.plugin.values().any(|entry| {
                    let update = entry.updates.borrow();
                    !entry.finished && update.attempt.package.name == package.name
                })
            {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "plugin account authorization is already active or capacity is exhausted",
                ));
            }
            Ok(flow)
        })();
        let flow = match prepared {
            Ok(flow) => flow,
            Err(error) => return database.finish_external(caller, request, Err(error), events),
        };
        let attempt = Attempt {
            id: request.id,
            package: package.clone(),
        };
        let result = database.finish_external(
            caller,
            request,
            Ok(Output::PluginLogin(attempt.clone())),
            events,
        )?;
        while self.plugin.len() >= 32 {
            let Some(key) = self
                .plugin
                .iter()
                .find(|(_, entry)| entry.finished)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.plugin.remove(&key);
        }
        let (updates, _) = watch::channel(LoginUpdate {
            attempt: attempt.clone(),
            revision: 1,
            state: State::Starting,
        });
        let (callback, returned) = oneshot::channel();
        let stop = self.closed.child_token();
        self.plugin.insert(
            (caller, request.id),
            Entry {
                updates,
                callback: Some(callback),
                stop: stop.clone(),
                finished: false,
            },
        );
        self.pending += 1;
        let sender = self.sender.clone();
        let service = self.oauth.clone();
        self.runtime.spawn(async move {
            let id = attempt.id;
            let operation = async {
                let expires_at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_err(storage_error)?.as_millis() as u64 + 900_000;
                sender.send(Job::Login(Box::new(super::Progress::Plugin(Progress::Prompt { caller, id, url: flow.url().into(), expires_at_ms })))).await.map_err(|_| super::super::unavailable())?;
                let callback = returned.await.map_err(|_| super::super::unavailable())?;
                flow.complete(&service, &callback).await
            };
            let result = tokio::select! {
                biased;
                _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "plugin account authorization cancelled")),
                result = tokio::time::timeout(Duration::from_secs(900), AssertUnwindSafe(operation).catch_unwind()) => result.ok().and_then(Result::ok).unwrap_or_else(|| Err(super::super::unavailable())),
            };
            let result = result.map(Authorized::Account);
            let _ = sender.send(Job::Login(Box::new(super::Progress::Plugin(Progress::Finished { caller, id, result })))).await;
        });
        Ok(result)
    }

    pub(super) fn plugin_progress(
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
                if let Some(entry) = self.plugin.get(&(caller, id))
                    && entry.updates.borrow().state.active()
                    && !entry.stop.is_cancelled()
                {
                    entry.set(State::Pending { url, expires_at_ms });
                }
            }
            Progress::Finished { caller, id, result } => {
                let Some(entry) = self.plugin.get_mut(&(caller, id)) else {
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
                let result = result.and_then(|grant| save(database, &attempt, grant, events));
                entry.set(match result {
                    Ok(package) => State::Connected(package),
                    Err(error) => State::Failed(error),
                });
            }
        }
    }

    pub(in crate::store) fn subscribe_plugin(
        &self,
        caller: NodeId,
        id: RequestId,
    ) -> Result<Box<dyn Subscription>, Fault> {
        let entry = self.plugin.get(&(caller, id)).ok_or_else(missing)?;
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
            Ok(Update::PluginLogin(
                self.receiver.borrow_and_update().clone(),
            ))
        })
    }
}

fn save(
    database: &mut Database,
    attempt: &Attempt,
    grant: Authorized,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<plugin::Reference, Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let info = match grant {
        Authorized::Account(grant) => oauth::save(&transaction, &attempt.package, grant)?,
        Authorized::Application(credentials, baseline) => {
            crate::store::plugins::registration::save(
                &transaction,
                &attempt.package,
                credentials,
                baseline,
            )?
        }
    };
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

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "plugin account authorization expired or belongs to another controller",
    )
}
