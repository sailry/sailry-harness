//! Device authorization shares durable admission and the Node's credential transaction owner.
mod mcp;
mod models;
mod persistence;
mod renewals;
use super::{Job, database::Database};
use crate::providers::login::{Grant, Service};
use futures::FutureExt;
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
    Mcp(mcp::Progress),
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
    finished: bool,
}

pub(super) struct Worker {
    pub pending: usize,
    entries: BTreeMap<(NodeId, RequestId), Entry>,
    mcp: BTreeMap<(NodeId, RequestId), mcp::Entry>,
    plugins: crate::plugins::Host,
    renewals: renewals::Renewals,
    service: Service,
    discovery: crate::providers::Discovery,
    runtime: Handle,
    sender: mpsc::Sender<Job>,
    closed: CancellationToken,
}

impl Worker {
    pub fn new(
        service: Service,
        discovery: crate::providers::Discovery,
        plugins: crate::plugins::Host,
        sender: mpsc::Sender<Job>,
        closed: CancellationToken,
    ) -> Self {
        Self {
            pending: 0,
            entries: BTreeMap::new(),
            mcp: BTreeMap::new(),
            plugins,
            renewals: Default::default(),
            service,
            discovery,
            runtime: Handle::current(),
            sender,
            closed,
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
        let result = match request.command {
            Command::BeginMcpLogin { .. }
            | Command::CompleteMcpLogin { .. }
            | Command::CancelMcpLogin { .. } => {
                self.mcp_command(database, caller, &request, events)
            }
            Command::BeginProviderLogin { .. } => self.begin(database, caller, &request, events),
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
            if matches!(
                provider.authentication,
                Authentication::ApiKey | Authentication::Host
            ) {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "provider does not use device authorization",
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
            Ok((provider, credential))
        })();
        let (provider, credential) = match prepared {
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
        self.entries.insert(
            (caller, request.id),
            Entry {
                provider,
                credential,
                updates,
                stop: stop.clone(),
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
                    .authorized(api, endpoint, &grant, stop.clone())
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
            Progress::Mcp(progress) => self.mcp_progress(database, progress, events),
            Progress::Renewed { id, result } => {
                if self.renewals.finish(database, id, result) {
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
