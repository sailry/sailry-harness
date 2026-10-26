//! Provider authorization shares durable admission and the Node's credential transaction owner.
mod connectors;
pub(in crate::store) mod model_auth;
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
    Connector(Box<connectors::Progress>),
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
    binding: Option<Box<crate::providers::login::plugin::Target>>,
}

struct Entry {
    plugin: Option<crate::providers::login::plugin::Capture>,
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
    connectors: BTreeMap<(NodeId, RequestId), connectors::Entry>,
    ingress: std::sync::Arc<Ingress>,
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
            connectors: BTreeMap::new(),
            ingress: std::sync::Arc::new(ingress.clone()),
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

    pub async fn execute(
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
        if matches!(request.command, Command::ProbeConnection { .. }) {
            self.probe_connector(database, caller, request, reply, events)
                .await;
            return;
        }
        if matches!(request.command, Command::InspectConnectorCli { .. }) {
            self.inspect_connector(database, caller, request, reply, events)
                .await;
            return;
        }
        let result = match request.command {
            Command::StartConnectorLogin { .. }
            | Command::SubmitConnectorInput { .. }
            | Command::CancelConnectorLogin { .. } => {
                self.connector_command(database, caller, &request, events)
                    .await
            }
            Command::BeginProviderLogin { .. } => {
                self.begin(database, caller, &request, events).await
            }
            Command::CompleteProviderLogin {
                attempt,
                ref callback,
            } => {
                self.complete(database, caller, &request, attempt, callback, events)
                    .await
            }
            Command::CancelProviderLogin { attempt } => {
                self.cancel(database, caller, &request, attempt, events)
                    .await
            }
            _ => unreachable!(),
        };
        let _ = reply.send(result);
    }

    async fn begin(
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
        let prepared = (async || {
            if self.closed.is_cancelled() {
                return Err(super::unavailable());
            }
            let provider =
                super::providers::current(&database.connection, provider, expected_revision)
                    .await?;
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
            let credential = (match provider.credential.as_ref() {
                Some(value) => Some(
                    async {
                        let reference: &CredentialRef = value;
                        super::providers::authentication::get(&database.connection, reference.id)
                            .await
                    }
                    .await,
                ),
                None => None,
            })
            .transpose()?
            .flatten();
            let plugin = if model_auth::managed(provider.authentication) {
                Some(
                    model_auth::prepare(
                        &database.connection,
                        &provider,
                        request.id,
                        redirect.as_deref(),
                    )
                    .await?,
                )
            } else {
                None
            };
            let options = match &plugin {
                Some(plugin) => plugin.configuration.options.clone(),
                None => crate::providers::login::effective(&provider)?,
            };
            let context = (match plugin.as_ref() {
                Some(value) => Some(
                    async {
                        let plugin: &model_auth::Prepared = value;
                        registration::prepare(
                            database,
                            &provider,
                            credential.as_ref(),
                            &plugin.configuration.target,
                        )
                        .await
                    }
                    .await,
                ),
                None => None,
            })
            .transpose()?;
            let adapter = if provider.api == sailry_protocol::conversation::ModelApi::Plugin {
                let mut captured = provider.clone();
                super::providers::presets::capture_read(&database.connection, &mut captured)
                    .await?;
                let declaration =
                    super::providers::adapters::read(&database.connection, &captured, false)
                        .await?;
                Some((captured, declaration))
            } else {
                None
            };
            Ok((provider, credential, options, context, plugin, adapter))
        })()
        .await;
        let (provider, credential, options, context, mut plugin, adapter) = match prepared {
            Ok(value) => value,
            Err(error) => {
                return database
                    .finish_external(caller, request, Err(error), events)
                    .await;
            }
        };
        let attempt = login::Attempt {
            id: request.id,
            provider: provider.id,
        };
        let output = database
            .finish_external(
                caller,
                request,
                Ok(Output::ProviderLogin(attempt.clone())),
                events,
            )
            .await?;
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
        let context = match context {
            Some(registration::Prepared {
                context,
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
                Some(context)
            }
            None => None,
        };
        let callback = plugin.as_mut().and_then(|plugin| plugin.returned.take());
        let binding = plugin
            .as_ref()
            .filter(|_| authentication != Authentication::Plugin)
            .map(|plugin| Box::new(plugin.configuration.target.clone()));
        self.entries.insert(
            (caller, request.id),
            Entry {
                plugin: plugin
                    .as_ref()
                    .map(|plugin| plugin.configuration.capture.clone()),
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
        let plugins = self.plugins.clone();
        let discovery = self.discovery.clone();
        let sender = self.sender.clone();
        let id = request.id;
        self.runtime.spawn(async move {
            let operation = async {
                let grant = if let Some(plugin) = plugin {
                    model_auth::authorize(
                        plugin,
                        service.clone(),
                        &plugins,
                        context,
                        |state| async {
                            sender
                                .send(Job::Login(Box::new(Progress::Prompt { caller, id, state })))
                                .await
                                .map_err(|_| super::unavailable())
                        },
                        stop.clone(),
                    )
                    .await?
                } else {
                    service
                        .authorize(
                            authentication,
                            &options,
                            |state| async {
                                sender
                                    .send(Job::Login(Box::new(Progress::Prompt {
                                        caller,
                                        id,
                                        state,
                                    })))
                                    .await
                                    .map_err(|_| super::unavailable())
                            },
                            &stop,
                        )
                        .await?
                };
                let models = if let Some((captured, declaration)) = adapter {
                    crate::agent::discover_authorized_models(
                        &plugins,
                        &captured,
                        &declaration,
                        &grant,
                        &options,
                        &stop,
                    )
                    .await
                    .map(|catalog| catalog.models)
                } else {
                    let endpoint = grant.endpoint();
                    #[cfg(any(test, feature = "test-support"))]
                    let endpoint = service.model_endpoint.as_deref().unwrap_or(endpoint);
                    discovery
                        .authorized(api, endpoint, &grant, &options, stop.clone())
                        .await
                };
                Ok(Authorized {
                    grant,
                    models,
                    binding,
                })
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

    async fn complete(
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
        let output = database
            .finish_external(caller, request, result, events)
            .await?;
        if let Some(entry) = self.entries.get_mut(&(caller, id))
            && let Some(sender) = entry.callback.take()
        {
            let _ = sender.send(callback.clone());
            entry.set(login::State::Exchanging);
        }
        Ok(output)
    }

    async fn cancel(
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
        let output = database
            .finish_external(caller, request, output, events)
            .await?;
        if let Some(entry) = self.entries.get_mut(&(caller, id)) {
            entry.stop.cancel();
            entry.callback.take();
            entry.set(login::State::Cancelled);
        }
        Ok(output)
    }

    pub async fn progress(
        &mut self,
        database: &mut Database,
        progress: Progress,
        events: &broadcast::Sender<EventEnvelope>,
    ) {
        match progress {
            Progress::Connector(progress) => {
                self.connector_progress(database, *progress, events).await
            }
            Progress::Registered {
                caller,
                id,
                client_id,
                reply,
            } => {
                let result = async {
                    let entry = self.entries.get(&(caller, id)).ok_or_else(missing)?;
                    if entry.stop.is_cancelled() {
                        return Err(Fault::new(
                            ErrorCode::Cancelled,
                            "provider authorization was cancelled",
                        ));
                    }
                    registration::record(
                        database,
                        &entry.provider,
                        entry.plugin.as_ref(),
                        &client_id,
                    )
                    .await
                }
                .await;
                let _ = reply.send(result);
            }
            Progress::Renewed { id, result } => {
                if self.renewals.finish(database, id, result, events).await {
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
                let mut stage = "authorization";
                let result = match result {
                    Ok(authorized) => {
                        async {
                            if let Some(capture) = &entry.plugin {
                                stage = "capture";
                                model_auth::complete(
                                    &database.connection,
                                    &entry.provider,
                                    capture,
                                )
                                .await?;
                            }
                            stage = "persistence";
                            persistence::save(
                                database,
                                &entry.provider,
                                entry.credential.as_ref(),
                                authorized,
                                events,
                            )
                            .await
                        }
                        .await
                    }
                    Err(error) => Err(error),
                };
                match result {
                    Ok(model_error) => entry.updates.send_modify(|update| {
                        update.revision += 1;
                        update.state = login::State::Connected;
                        update.model_error = model_error;
                    }),
                    Err(error) => {
                        let mut record = crate::logging::Entry::new(
                            crate::logging::Area::Node,
                            crate::logging::Phase::Failed,
                        );
                        record.operation = Some("provider_authorization");
                        record.stage = Some(stage);
                        record.context.provider = Some(entry.provider.id);
                        record.context.request = Some(id);
                        record.code = Some(error.code);
                        record.emit();
                        entry.set(login::State::Failed(error));
                    }
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
