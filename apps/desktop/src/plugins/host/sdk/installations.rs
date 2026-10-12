//! Observation is restricted to this Host's captured installation request IDs.
use super::*;
use sailry_protocol::installation::{self, Task};

pub(in crate::plugins::host) struct Subscription {
    scope: Mutex<Option<Scope>>,
    pending: Arc<tokio::sync::Semaphore>,
}
impl Default for Subscription {
    fn default() -> Self {
        Self {
            scope: Default::default(),
            pending: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}

struct Scope {
    stop: CancellationToken,
    updates: tokio::sync::watch::Receiver<Value>,
    first: bool,
}
impl Drop for Scope {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Host {
    pub(in crate::plugins) fn installations_module(self: &Arc<Self>) -> HostModule {
        let read = self.clone();
        let watch = self.clone();
        let next = self.clone();
        let dismiss = self.clone();
        HostModule::new("sailry/installations")
            .async_function("read", move |args| {
                let ids = ids(args.value(0)?)?;
                read.read_installations(ids)
            })
            .function("watch", move |args| {
                let ids = ids(args.value(0)?)?;
                watch.watch_installations(ids)?;
                Ok(HostValue::Null)
            })
            .async_function("next", move |_| next.next_installations())
            .async_function("dismiss", move |args| {
                let id = args
                    .string(0)?
                    .parse()
                    .map_err(|_| HostError::new("invalid installation request ID"))?;
                let (node, controller) = dismiss.owned_installations(&[id])?;
                let owner = dismiss.clone();
                Ok(async move {
                    if !controller.is_empty() {
                        owner
                            .controller_manager()?
                            .dismiss(id)
                            .map_err(HostError::new)?;
                    } else {
                        debug_assert_eq!(node, vec![id]);
                        owner
                            .read_public(Command::DismissInstallation { request: id })?
                            .await?;
                    }
                    Ok(HostValue::Null)
                })
            })
            .declarations(include_str!("installations.d.ts"))
    }

    fn owned_installations(
        &self,
        ids: &[RequestId],
    ) -> Result<(Vec<RequestId>, Vec<RequestId>), HostError> {
        self.check()?;
        installation::validate_requests(ids).map_err(values::fault)?;
        let controller = self.controller_requests(ids)?;
        let drafts = self.requests.lock().map_err(lock_error)?;
        let scope = self.context();
        let mut node = Vec::new();
        for id in ids {
            if controller.contains(id) {
                continue;
            }
            let request = drafts
                .get(id)
                .filter(|request| {
                    matches!(
                        request.command,
                        Command::InstallOcrModel { .. }
                            | Command::InstallKnowledgeModel { .. }
                            | Command::InstallCodeServer { .. }
                    ) && request.plugin.as_ref() == Some(&scope)
                })
                .ok_or_else(|| {
                    values::fault(sailry_protocol::Fault::new(
                        sailry_protocol::ErrorCode::PermissionDenied,
                        "installation request is not owned by this plugin view",
                    ))
                })?;
            node.push(request.id);
        }
        Ok((node, controller))
    }

    fn read_installations(
        self: &Arc<Self>,
        ids: Vec<RequestId>,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        let (node, controller) = self.owned_installations(&ids)?;
        let owner = self.clone();
        Ok(async move {
            let mut rows = if node.is_empty() {
                Vec::new()
            } else {
                decode(
                    &owner
                        .read_public(Command::ReadInstallations { requests: node })?
                        .await?,
                )?
                .as_array()
                .cloned()
                .ok_or_else(|| HostError::new("invalid installation response"))?
            };
            if !controller.is_empty() {
                rows.extend(controller_rows(
                    &owner.controller_manager()?.installations(),
                    &controller,
                ));
            }
            encode(Value::Array(rows))
        })
    }

    fn watch_installations(self: &Arc<Self>, ids: Vec<RequestId>) -> Result<(), HostError> {
        let (node, controller) = self.owned_installations(&ids)?;
        let manager = (!controller.is_empty())
            .then(|| self.controller_manager())
            .transpose()?;
        let stop = CancellationToken::new();
        let (sender, updates) = tokio::sync::watch::channel(Value::Array(Vec::new()));
        let mut scope = self.installations.scope.lock().map_err(lock_error)?;
        *scope = Some(Scope {
            stop: stop.clone(),
            updates,
            first: true,
        });
        drop(scope);
        let owner = self.clone();
        self.runtime.spawn(async move {
            let (node_sender, mut node_updates) = tokio::sync::watch::channel(sailry_client::installation::View::default());
            let node_stop = stop.clone();
            if !node.is_empty() {
                let request = owner.client.prepare(Command::ReadInstallations { requests: node }).with_plugin(owner.context());
                let client = owner.client.clone();
                owner.runtime.spawn(async move { let _ = client.watch_installations_for(request, node_sender, node_stop).await; });
            }
            let mut controller_updates = manager.as_ref().map(|manager| manager.subscribe());
            loop {
                let tasks = node_updates.borrow_and_update().tasks.clone();
                let mut rows = match public_output(Output::Installations(tasks)) {
                    Ok(value) => value["data"].as_array().cloned().unwrap_or_default(),
                    Err(_) => break,
                };
                if let Some(manager) = &manager { rows.extend(controller_rows(&manager.installations(), &controller)); }
                sender.send_replace(Value::Array(rows));
                tokio::select! {
                    biased;
                    _ = owner.stop.cancelled() => break,
                    _ = stop.cancelled() => break,
                    changed = node_updates.changed(), if !node_updates.has_changed().is_err() => if changed.is_err() && manager.is_none() { break; },
                    changed = async {
                        match &mut controller_updates {
                            Some(updates) => updates.changed().await,
                            None => std::future::pending().await,
                        }
                    } => if changed.is_err() { break; },
                }
            }
            stop.cancel();
        });
        Ok(())
    }

    fn next_installations(
        self: &Arc<Self>,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        self.check()?;
        let permit = self
            .installations
            .pending
            .clone()
            .try_acquire_owned()
            .map_err(|_| HostError::new("an installation update request is already pending"))?;
        let (mut updates, first, stop) = {
            let mut scope = self.installations.scope.lock().map_err(lock_error)?;
            let scope = scope
                .as_mut()
                .ok_or_else(|| HostError::new("installation observation has not started"))?;
            let first = scope.first;
            scope.first = false;
            (scope.updates.clone(), first, scope.stop.clone())
        };
        let owner = self.clone();
        Ok(async move {
            let _permit = permit;
            if !first {
                tokio::select! {
                    biased;
                    _ = owner.stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                    _ = stop.cancelled() => return Err(HostError::new("installation observation changed")),
                    changed = updates.changed() => changed.map_err(|_| HostError::new("installation observation ended"))?,
                }
            }
            let value = updates.borrow_and_update().clone();
            if let Some(scope) = owner
                .installations
                .scope
                .lock()
                .map_err(lock_error)?
                .as_mut()
                && !stop.is_cancelled()
            {
                scope.updates = updates;
            }
            encode(value)
        })
    }
}

fn ids(value: &HostValue) -> Result<Vec<RequestId>, HostError> {
    serde_json::from_value(decode(value)?)
        .map_err(|_| HostError::new("invalid installation request IDs"))
}

fn controller_rows(tasks: &[Task], ids: &[RequestId]) -> Vec<Value> {
    tasks
        .iter()
        .filter(|task| ids.contains(&task.request))
        .map(|task| {
            let mut value = json!(task);
            value["revision"] = json!(task.revision.to_string());
            value["target"] = json!("controller");
            value
        })
        .collect()
}
