//! Provider authorization shares durable admission and the Node's credential transaction owner.
#[path = "login/plugin.rs"]
mod account;
mod github;
mod mcp;
mod models;
mod persistence;
mod registration;
mod renewals;
use super::{Ingress, Job, database::Database};
use crate::providers::login::{Grant, Service};
use futures::FutureExt;
pub(crate) use renewals::Authorization;
use sailry_link::{CancellationToken, Pending, Response, Subscription};
use sailry_protocol::{
    conversation::{Provider, discovery, login},
    *,
};
use std::{collections::BTreeMap, panic::AssertUnwindSafe};
use tokio::{
    runtime::Handle,
    sync::{broadcast, mpsc, oneshot, watch},
};

pub(super) struct Operation {
    pub caller: NodeId,
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}

pub(super) enum Progress {
    Github(Box<github::Completed>),
    Mcp(mcp::Progress),
    Plugin(account::Progress),
    Registered {
        caller: NodeId,
        id: RequestId,
        client_id: String,
        reply: oneshot::Sender<Result<(), Fault>>,
    },
    Renewed {
        id: CredentialId,
        result: Result<Grant, Fault>,
    },
    Prompt {
        caller: NodeId,
        id: RequestId,
        state: login::State,
    },
    Finished {
        caller: NodeId,
        id: RequestId,
        result: Result<Authorized, Fault>,
    },
}

pub(super) struct Authorized {
    grant: Grant,
    models: Result<Vec<discovery::Model>, Fault>,
}

struct Entry {
    provider: Provider,
    credential: Option<Credential>,
    updates: watch::Sender<login::Update>,
    stop: CancellationToken,
    callback: Option<oneshot::Sender<Secret>>,
    finished: bool,
}

pub(super) struct Worker {
    pub pending: usize,
    entries: BTreeMap<(NodeId, RequestId), Entry>,
    mcp: BTreeMap<(NodeId, RequestId), mcp::Entry>,
    plugin: BTreeMap<(NodeId, RequestId), account::Entry>,
    oauth: std::sync::Arc<crate::plugins::oauth::Service>,
    plugins: crate::plugins::Host,
    environment: std::sync::Arc<crate::process::Environment>,
    renewals: renewals::Renewals,
    service: Service,
    discovery: crate::providers::Discovery,
    runtime: Handle,
    sender: mpsc::Sender<Job>,
    events: broadcast::Sender<EventEnvelope>,
    closed: CancellationToken,
}

impl Worker {
    pub fn new(
        service: Service,
        ingress: &Ingress,
        events: broadcast::Sender<EventEnvelope>,
    ) -> Self {
        Self {
            pending: 0,
            entries: BTreeMap::new(),
            mcp: BTreeMap::new(),
            plugin: BTreeMap::new(),
            oauth: ingress.oauth.clone(),
            plugins: ingress.plugins.clone(),
            environment: ingress.environment.clone(),
            renewals: Default::default(),
            service,
            discovery: ingress.discovery.clone(),
            runtime: Handle::current(),
            sender: ingress.sender.clone(),
            events,
            closed: ingress.closed.clone(),
        }
    }

    pub fn execute(
        &mut self,
        database: &mut Database,
        operation: Operation,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        let Operation {
            caller,
            request,
            reply,
        } = operation;
        if matches!(request.command, Command::ImportGithubLogin { .. }) {
            self.import_github(database, caller, request, reply, events);
            return;
        }
        let result = match request.command {
            Command::BeginPluginLogin { .. }
            | Command::CompletePluginLogin { .. }
            | Command::CancelPluginLogin { .. } => {
                self.plugin_command(database, caller, &request, events)
            }
            Command::BeginMcpLogin { .. }
            | Command::CompleteMcpLogin { .. }
            | Command::CancelMcpLogin { .. } => {
                self.mcp_command(database, caller, &request, events)
            }
            Command::BeginProviderLogin { .. } => self.begin(database, caller, &request, events),
            Command::CompleteProviderLogin {
                attempt,
                ref callback,
            } => self.complete(database, caller, &request, attempt, callback, events),
            Command::CancelProviderLogin { attempt } => {
                self.cancel(database, caller, &request, attempt, events)
            }
            _ => unreachable!(),
        };
        let _ = reply.send(result);
    }

    fn begin(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        let Command::BeginProviderLogin {
            provider,
            expected_revision,
            ref redirect,
        } = request.command
        else {
            unreachable!()
        };
        let prepared = (|| {
            if self.closed.is_cancelled() {
                return Err(super::unavailable());
            }
            let provider =
                super::providers::current(&database.connection, provider, expected_revision)?;
            crate::providers::login::validate(&provider)?;
            if provider.authentication == Authentication::ApiKey {
                return Err(Fault::invalid(
                    "provider does not use browser authorization",
                ));
            }
            if self
                .entries
                .values()
                .filter(|entry| !entry.finished)
                .count()
                >= 4
                || self
                    .entries
                    .values()
                    .any(|entry| !entry.finished && entry.provider.id == provider.id)
            {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "provider authorization is already active or capacity is exhausted",
                ));
            }
            let credential = provider
                .credential
                .as_ref()
                .map(|reference| {
                    super::providers::authentication::get(&database.connection, reference.id)
                })
                .transpose()?
                .flatten();
            let options = crate::providers::login::effective(&provider)?;
            let context = registration::prepare(
                database,
                &provider,
                credential.as_ref(),
                redirect.as_deref(),
            )?;
            Ok((provider, credential, options, context))
        })();
        let (provider, credential, options, context) = match prepared {
            Ok(value) => value,
            Err(error) => return database.finish_external(caller, request, Err(error), events),
        };
        let attempt = login::Attempt {
            id: request.id,
            provider: provider.id,
        };
        let output = database.finish_external(
            caller,
            request,
            Ok(Output::ProviderLogin(attempt.clone())),
            events,
        )?;
        // Persist the attempt identifier before starting HTTP. Receipt recovery never creates a second login.
        while self.entries.len() >= 32 {
            let Some(key) = self
                .entries
                .iter()
                .find(|(_, entry)| entry.finished)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.entries.remove(&key);
        }
        let (updates, _) = watch::channel(login::Update {
            attempt,
            revision: 1,
            state: login::State::Starting,
            model_error: None,
        });
        let stop = self.closed.child_token();
        let authentication = provider.authentication;
        let api = provider.api;
        let (authorization, callback) = match context {
            Some(registration::Prepared {
                authorization,
                callback,
                registered,
            }) => {
                let sender = self.sender.clone();
                let id = request.id;
                self.runtime.spawn(async move {
                    if let Ok((client_id, reply)) = registered.await {
                        let _ = sender
                            .send(Job::Login(Box::new(Progress::Registered {
                                caller,
                                id,
                                client_id,
                                reply,
                            })))
                            .await;
                    }
                });
                (Some(authorization), Some(callback))
            }
            None => (None, None),
        };
        self.entries.insert(
            (caller, request.id),
            Entry {
                provider,
                credential,
                updates,
                stop: stop.clone(),
                callback,
                finished: false,
            },
        );
        self.pending += 1;
        let service = self.service.clone();
        let discovery = self.discovery.clone();
        let sender = self.sender.clone();
        let id = request.id;
        self.runtime.spawn(async move {
            let operation = async {
                let grant = service
                    .authorize(
                        authentication,
                        &options,
                        authorization,
                        |state| async {
                            sender
                                .send(Job::Login(Box::new(Progress::Prompt { caller, id, state })))
                                .await
                                .map_err(|_| super::unavailable())
                        },
                        &stop,
                    )
                    .await?;
                let endpoint = grant.endpoint();
                #[cfg(any(test, feature = "test-support"))]
                let endpoint = service.model_endpoint.as_deref().unwrap_or(endpoint);
                let models = discovery
                    .authorized(api, endpoint, &grant, &options, stop.clone())
                    .await;
                Ok(Authorized { grant, models })
            };
            let result = AssertUnwindSafe(operation)
                .catch_unwind()
                .await
                .unwrap_or_else(|_| Err(super::unavailable()));
            let _ = sender
                .send(Job::Login(Box::new(Progress::Finished {
                    caller,
                    id,
                    result,
                })))
                .await;
        });
        Ok(output)
    }

    fn complete(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        id: RequestId,
        callback: &Secret,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        let result = self
            .entries
            .get(&(caller, id))
            .ok_or_else(missing)
            .and_then(|entry| {
                if !matches!(entry.updates.borrow().state, login::State::Browser { .. })
                    || entry.callback.is_none()
                {
                    return Err(Fault::new(
                        ErrorCode::Conflict,
                        "provider authorization is not awaiting a callback",
                    ));
                }
                if callback.expose().len() > 16 * 1024
                    || callback.expose().chars().any(char::is_control)
                {
                    return Err(Fault::invalid("authorization callback is invalid"));
                }
                Ok(Output::ProviderLogin(
                    entry.updates.borrow().attempt.clone(),
                ))
            });
        let output = database.finish_external(caller, request, result, events)?;
        if let Some(entry) = self.entries.get_mut(&(caller, id))
            && let Some(sender) = entry.callback.take()
        {
            let _ = sender.send(callback.clone());
            entry.set(login::State::Exchanging);
        }
        Ok(output)
    }

    fn cancel(
        &mut self,
        database: &mut Database,
        caller: NodeId,
        request: &Request,
        id: RequestId,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        let output = self
            .entries
            .get(&(caller, id))
            .ok_or_else(missing)
            .and_then(|entry| {
                if entry.updates.borrow().state.active() {
                    Ok(Output::ProviderLogin(
                        entry.updates.borrow().attempt.clone(),
                    ))
                } else {
                    Err(Fault::new(
                        ErrorCode::Conflict,
                        "provider authorization has already finished",
                    ))
                }
            });
        let output = database.finish_external(caller, request, output, events)?;
        if let Some(entry) = self.entries.get_mut(&(caller, id)) {
            entry.stop.cancel();
            entry.callback.take();
            entry.set(login::State::Cancelled);
        }
        Ok(output)
    }

    pub fn progress(
        &mut self,
        database: &mut Database,
        progress: Progress,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        match progress {
            Progress::Github(completed) => self.github_progress(database, *completed, events),
            Progress::Mcp(progress) => self.mcp_progress(database, progress, events),
            Progress::Plugin(progress) => self.plugin_progress(database, progress, events),
            Progress::Registered {
                caller,
                id,
                client_id,
                reply,
            } => {
                let result = self.entries.get(&(caller, id)).ok_or_else(missing).and_then(|entry| {
                    if entry.stop.is_cancelled() {
                        return Err(Fault::new(ErrorCode::Cancelled, "provider authorization was cancelled"));
                    }
                    database.connection.execute(
                        "INSERT INTO oauth_registrations(provider,client_id) VALUES(?1,?2) ON CONFLICT(provider) DO UPDATE SET client_id=excluded.client_id",
                        rusqlite::params![entry.provider.id.to_string(), client_id],
                    ).map(|_| ()).map_err(super::database::storage_error)
                });
                let _ = reply.send(result);
            }
            Progress::Renewed { id, result } => {
                if self.renewals.finish(database, id, result, events) {
                    self.pending -= 1;
                }
            }
            Progress::Prompt { caller, id, state } => {
                if let Some(entry) = self.entries.get_mut(&(caller, id))
                    && entry.updates.borrow().state.active()
                    && !entry.stop.is_cancelled()
                {
                    entry.set(state);
                }
            }
            Progress::Finished { caller, id, result } => {
                let Some(entry) = self.entries.get_mut(&(caller, id)) else {
                    return;
                };
                if entry.finished {
                    return;
                }
                entry.finished = true;
                entry.callback.take();
                self.pending -= 1;
                if !entry.updates.borrow().state.active() {
                    return;
                }
                if entry.stop.is_cancelled() {
                    entry.set(login::State::Cancelled);
                    return;
                }
                let result = result.and_then(|authorized| {
                    persistence::save(
                        database,
                        &entry.provider,
                        entry.credential.as_ref(),
                        authorized,
                        events,
                    )
                });
                match result {
                    Ok(model_error) => entry.updates.send_modify(|update| {
                        update.revision += 1;
                        update.state = login::State::Connected;
                        update.model_error = model_error;
                    }),
                    Err(error) => entry.set(login::State::Failed(error)),
                }
            }
        }
    }

    pub fn subscribe(&self, caller: NodeId, id: RequestId) -> Result<Box<dyn Subscription>, Fault> {
        let entry = self.entries.get(&(caller, id)).ok_or_else(missing)?;
        Ok(Box::new(Updates {
            receiver: entry.updates.subscribe(),
            initial: true,
            closed: self.closed.clone(),
        }))
    }
}

impl Entry {
    fn set(&self, state: login::State) {
        self.updates.send_modify(|update| {
            update.revision += 1;
            update.state = state;
        });
    }
}

struct Updates {
    receiver: watch::Receiver<login::Update>,
    initial: bool,
    closed: CancellationToken,
}
impl Subscription for Updates {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if !self.initial {
                tokio::select! {
                    biased;
                    _ = self.closed.cancelled() => return Err(super::unavailable()),
                    result = self.receiver.changed() => result.map_err(|_| missing())?,
                }
            }
            self.initial = false;
            Ok(Update::ProviderLogin(
                self.receiver.borrow_and_update().clone(),
            ))
        })
    }
}

fn missing() -> Fault {
    Fault::new(
        ErrorCode::NotFound,
        "provider authorization expired or belongs to another controller",
    )
}
