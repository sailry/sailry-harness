//! A request keeps Node task ownership without serializing ordinary SQL behind execution workers.
use super::*;

impl Ingress {
    pub(super) fn persist_request(
        &self,
        access: crate::files::Access,
        caller: NodeId,
        request: Box<Request>,
        reply: oneshot::Sender<Result<Admission, Fault>>,
        order: Option<tokio::sync::OwnedMutexGuard<()>>,
    ) -> Pending<'_, Option<external::Work>> {
        Box::pin(async move {
            let Some(storage) = self.storage.get() else {
                let _ = reply.send(Err(unavailable()));
                return None;
            };
            let mut database = match storage.context(false).await {
                Ok(database) => database,
                Err(error) => {
                    let _ = reply.send(Err(database::storage_error(error)));
                    return None;
                }
            };
            let events = &self.events;
            let accepting = &self.accepting;
            let agents = &self.agents;
            let cleanup_commands = &self.commands;
            let attachment_profile = &self.profile;
            let backup_resources = &self.backup_resources;
            let cleanup_runtime = tokio::runtime::Handle::current();
            let code_index = &self.code_index;
            let searcher = &self.search;
            let power = &self.power;

            let mut work = None;
            let (reply, artifact_cleanup) = if matches!(
                request.command,
                sailry_protocol::Command::RemoveSession { .. }
                    | sailry_protocol::Command::RemoveSessions { .. }
            ) {
                let (send, cleanup) = oneshot::channel();
                (
                    admission::release_removed(
                        cleanup_commands.clone(),
                        attachment_profile.clone(),
                        backup_resources.clone(),
                        &cleanup_runtime,
                        reply,
                        cleanup,
                    ),
                    Some(send),
                )
            } else {
                (reply, None)
            };
            let (reply, power_completion) = if matches!(
                request.command,
                sailry_protocol::Command::SavePowerSettings(_)
            ) {
                let (complete, response) = oneshot::channel();
                (complete, Some((reply, response)))
            } else {
                (reply, None)
            };
            let code_settings = code_index::configures(&request.command);
            let (reply, code_completion) = if code_settings {
                let (complete, response) = oneshot::channel();
                (complete, Some((reply, response)))
            } else {
                (reply, None)
            };
            let search_settings = matches!(
                request.command,
                sailry_protocol::Command::SaveSearchSettings(_)
            );
            let launch = super::routing::launches(&request.command);
            let sending = match request.command {
                sailry_protocol::Command::SendQueuedTurn { turn, .. } => Some(turn),
                _ => None,
            };
            let stopping = match &request.command {
                sailry_protocol::Command::StopTurn { turn } => Some(*turn),
                sailry_protocol::Command::StopDispatchTurn { package, job } => {
                    dispatch::jobs::turn(&database.connection, &package.name, *job)
                        .await
                        .ok()
                }
                _ => None,
            };
            let transaction_stops = match &request.command {
                sailry_protocol::Command::PluginTransaction { operations } => operations
                    .iter()
                    .filter_map(|operation| {
                        if let sailry_protocol::plugin::transaction::Operation::Stop { turn } =
                            operation
                        {
                            Some(*turn)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            if !accepting.load(Ordering::Acquire) {
                let _ = reply.send(Err(unavailable()));
            } else {
                work = database
                    .dispatch_with(caller, *request, reply, events, access)
                    .await;
            }
            if let Some(cleanup) = artifact_cleanup {
                let _ = cleanup.send(artifacts::cleanup(&database.connection).await);
            }
            if let Some((reply, completion)) = code_completion {
                let result = completion.await.unwrap_or_else(|_| Err(unavailable()));
                if result.is_ok() {
                    code_index::refresh(&database.connection, code_index).await;
                }
                let _ = reply.send(result);
            }
            if let Some((reply, completion)) = power_completion {
                let mut result = completion.await.unwrap_or_else(|_| Err(unavailable()));
                if result.is_ok() {
                    // Durable replay may contain an old saved value. Apply only current storage.
                    match power::read(&database.connection).await {
                        Ok(settings) => power.apply_settings(settings),
                        Err(error) => result = Err(error),
                    }
                }
                let _ = reply.send(result);
            }
            if search_settings && let Ok(settings) = search::read(&database.connection).await {
                searcher.configure(settings);
            }
            if let Some(turn) = stopping
                && agent::stopping(&database.connection, turn).await
            {
                agents.cancel(turn);
            }
            for turn in transaction_stops {
                if agent::stopping(&database.connection, turn).await {
                    agents.cancel(turn);
                }
            }
            if let Some(selected) = sending
                && let Ok(turns) = agent::queue::stopping(&database.connection, selected).await
            {
                for turn in turns {
                    agents.cancel(turn);
                }
            }
            if launch {
                agents.wake.notify_waiters();
            }

            drop(database);
            drop(order);
            work
        })
    }

    /// Called only outside the receiver that owns mutable execution services.
    pub(super) async fn handoff(&self, work: external::Work) {
        if let Some(storage) = self.storage.get()
            && let Err(error) = self.sender.send(Job::Execute(Box::new(work))).await
        {
            let Job::Execute(work) = error.0 else {
                unreachable!()
            };
            let (caller, request, reply) = external::reject(*work);
            if let Ok(mut database) = storage.context(false).await {
                let result = database
                    .finish_external(caller, &request, Err(unavailable()), &self.events)
                    .await;
                let _ = reply.send(result);
            }
        }
    }
}
