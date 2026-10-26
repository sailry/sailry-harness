//! Request routing shares authority checks; only internal callbacks await Store capacity.
use super::{Ingress, Job, unavailable};
use sailry_link::{Admission, Pending};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request};
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::oneshot;
pub(super) fn release_removed(
    commands: Arc<crate::process::background::Commands>,
    profile: Option<std::path::PathBuf>,
    backup_resources: Arc<tokio::sync::RwLock<()>>,
    runtime: &tokio::runtime::Handle,
    reply: oneshot::Sender<Result<Admission, Fault>>,
    cleanup: oneshot::Receiver<Result<Vec<sailry_protocol::SessionId>, Fault>>,
) -> oneshot::Sender<Result<Admission, Fault>> {
    let (send, received) = oneshot::channel::<Result<Admission, Fault>>();
    runtime.spawn(async move {
        match received.await {
            Ok(Ok(mut admission)) => {
                let (completed, completion) = oneshot::channel();
                let original = std::mem::replace(&mut admission.completion, completion);
                // Receipt and cleanup have separate lifetimes. Store ownership
                // releases removed handles even if the caller never receives it.
                let _ = reply.send(Ok(admission));
                if let Ok(result) = original.await {
                    if let Ok(sailry_protocol::Output::SessionsRemoved(sessions)) = &result {
                        let owners = match cleanup.await {
                            Ok(Ok(owners)) => owners,
                            Ok(Err(error)) => {
                                eprintln!("removed artifact cleanup plan failed: {error}");
                                Vec::new()
                            }
                            Err(error) => {
                                eprintln!("removed artifact cleanup plan unavailable: {error}");
                                Vec::new()
                            }
                        };
                        for session in sessions {
                            commands.close_session(*session).await;
                        }
                        if let Some(profile) = &profile
                            && !owners.is_empty()
                        {
                            let profile = profile.clone();
                            let resources = backup_resources.write_owned().await;
                            match tokio::task::spawn_blocking(move || {
                                let _resources = resources;
                                let mut result = Ok(());
                                for owner in owners {
                                    let cleanup = crate::files::artifacts::remove(&profile, owner);
                                    result = result.and(cleanup);
                                }
                                result
                            })
                            .await
                            {
                                Ok(Ok(())) => {}
                                Ok(Err(error)) => {
                                    eprintln!("removed artifact directory cleanup failed: {error}")
                                }
                                Err(error) => {
                                    eprintln!("removed artifact cleanup worker failed: {error}")
                                }
                            }
                        }
                    }
                    let _ = completed.send(result);
                }
            }
            Ok(Err(error)) => {
                let _ = reply.send(Err(error));
            }
            Err(_) => {}
        }
    });
    send
}
impl Ingress {
    pub(super) async fn read_request(
        &self,
        caller: NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        let id = request.id;
        let (reply, completion) = oneshot::channel();
        self.submit(Job::Read {
            caller,
            request: Box::new(request),
            reply,
        })
        .await?;
        Ok(Admission {
            receipt: sailry_protocol::Receipt { id, durable: false },
            completion,
        })
    }
    pub(crate) async fn dispatch_internal(&self, request: Request) -> Result<Admission, Fault> {
        self.dispatch_request(self.node, request, true).await
    }
    pub(super) fn submit(&self, job: Job) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async move {
            if super::operations::accepts(&job) {
                let ingress = self.clone();
                self.operations
                    .spawn_child(&sailry_link::CancellationToken::new(), async move {
                        super::operations::execute(ingress, job).await;
                    })
                    .await
                    .map_err(super::database::storage_error)?;
                Ok(())
            } else {
                self.sender.send(job).await.map_err(|_| unavailable())
            }
        })
    }
    pub(super) async fn send_internal(&self, job: Job) -> Result<(), Fault> {
        self.submit(job).await
    }
    pub(super) fn dispatch_request(
        &self,
        caller: NodeId,
        request: Request,
        internal: bool,
    ) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let mut span = crate::logging::commands::begin(self.node, caller, &request);
            let result = self.route_request(caller, request, internal).await;
            if let (Some(span), Err(fault)) = (span.as_mut(), &result) {
                crate::logging::commands::rejected(span, fault);
            }
            result.map(|admission| match span {
                Some(span) => crate::logging::commands::admitted(span, admission),
                None => admission,
            })
        })
    }
    fn route_request(
        &self,
        caller: NodeId,
        request: Request,
        internal: bool,
    ) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(
                request.command,
                sailry_protocol::Command::DeliverBackground { .. }
            ) {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "background delivery is internal Node input",
                ));
            }
            if !self.accepting.load(Ordering::Acquire) {
                return Err(unavailable());
            }
            super::routing::validate(self.node, &request)?;
            let route = super::routing::route(&request.command);
            match route {
                super::routing::Route::ReadNetwork => {
                    return defer(move || self.read_network(request)).await;
                }
                super::routing::Route::ListChannelPresets => {
                    return defer(move || self.list_channel_presets(caller, request)).await;
                }
                super::routing::Route::Read => {
                    return defer(move || self.read_request(caller, request)).await;
                }
                _ => {}
            }
            if request.plugin.is_some() && !request.command.durable() {
                self.check_plugin(caller, request.clone()).await?;
            }
            match route {
                super::routing::Route::ReadInstallations => {
                    return self.read_installations(caller, request);
                }
                super::routing::Route::DismissInstallation => {
                    return self.dismiss_installation(caller, request);
                }
                super::routing::Route::ReadBackup => {
                    return defer(move || self.read_backup(request)).await;
                }
                super::routing::Route::ReadArtifact => {
                    return defer(move || self.read_artifact(caller, request, internal)).await;
                }
                super::routing::Route::FetchWeb => {
                    return defer(move || self.fetch_web(caller, request)).await;
                }
                super::routing::Route::SearchWeb => {
                    return defer(move || self.search_web(caller, request)).await;
                }
                super::routing::Route::ModelCatalog => {
                    return defer(move || self.model_catalog(request)).await;
                }
                super::routing::Route::ReadStorageSearch if self.knowledge.get().is_some() => {
                    return defer(move || self.read_storage_search(request)).await;
                }
                super::routing::Route::ReadKnowledge => {
                    return defer(move || self.read_knowledge(request)).await;
                }
                super::routing::Route::Decide => return defer(move || self.decide(request)).await,
                super::routing::Route::ReadLocalModels => {
                    return defer(move || self.read_local_models(request)).await;
                }
                super::routing::Route::ReadCodeIndex => {
                    return defer(move || self.read_code_index(caller, request)).await;
                }
                super::routing::Route::ReadCommand => return self.read_command(request),
                super::routing::Route::Port => {
                    return defer(move || self.port(caller, request)).await;
                }
                super::routing::Route::ReadPluginMcp => {
                    return defer(move || self.read_plugin_mcp(request)).await;
                }
                super::routing::Route::ReadPluginView => {
                    return defer(move || self.read_plugin_view(request)).await;
                }
                super::routing::Route::CheckPluginUpdate => {
                    return defer(move || self.check_plugin_update(request)).await;
                }
                super::routing::Route::PluginCatalog => {
                    return defer(move || self.plugin_catalog(request)).await;
                }
                super::routing::Route::DiscoverSkills => {
                    return defer(move || self.discover_skills(caller, request, internal)).await;
                }
                super::routing::Route::RequestProviderHttp => {
                    return defer(move || self.request_provider_http(caller, request)).await;
                }
                super::routing::Route::DiscoverModels => {
                    return defer(move || self.discover_models(request)).await;
                }
                super::routing::Route::RefreshCatalog => {
                    return defer(move || self.refresh_catalog(request)).await;
                }
                super::routing::Route::Terminal => {
                    return defer(move || self.terminal(caller, request)).await;
                }
                super::routing::Route::Transfer => {
                    return defer(move || self.transfer(caller, request, internal)).await;
                }
                super::routing::Route::InspectWorktree => {
                    return defer(move || self.inspect_worktree(request, internal)).await;
                }
                _ => {}
            }
            if route == super::routing::Route::Browser {
                let sailry_protocol::Command::CompleteBrowser { id, result } = &request.command
                else {
                    unreachable!("browser completion route expected")
                };
                self.browsers.complete(caller, *id, result.clone())?;
                let (send, completion) = oneshot::channel();
                let _ = send.send(Ok(sailry_protocol::Output::BrowserCompleted));
                return Ok(Admission {
                    receipt: sailry_protocol::Receipt {
                        id: request.id,
                        durable: false,
                    },
                    completion,
                });
            }
            if route == super::routing::Route::Host {
                let output = match request.command {
                    sailry_protocol::Command::ReadComputerPermissions
                    | sailry_protocol::Command::RequestComputerPermission { .. } => {
                        let node = self.node;
                        let permission = match &request.command {
                            sailry_protocol::Command::RequestComputerPermission { permission } => {
                                Some(*permission)
                            }
                            _ => None,
                        };
                        if permission.is_some() && node != caller {
                            return Err(Fault::new(
                                ErrorCode::PermissionDenied,
                                "system permissions must be granted on the execution device",
                            ));
                        }
                        #[cfg(target_os = "macos")]
                        let launch = {
                            if request.plugin.is_none() {
                                return Err(crate::computer::permissions::unconfigured());
                            }
                            let (info, settings) =
                                self.computer_inputs(caller, request.clone()).await?;
                            let host = self.plugins.clone();
                            let stop = self.closed.clone();
                            Some(
                                tokio::task::spawn_blocking(move || {
                                    host.computer_launch(&info, &settings, stop)
                                })
                                .await
                                .map_err(|_| unavailable())??,
                            )
                        };
                        #[cfg(not(target_os = "macos"))]
                        let launch = None;
                        let permissions = if let Some(permission) = permission {
                            crate::computer::permissions::request(
                                node,
                                caller,
                                permission,
                                launch,
                                &self.environment,
                                &self.closed,
                            )
                            .await?
                        } else {
                            crate::computer::permissions::read(
                                node,
                                caller,
                                launch,
                                &self.environment,
                                &self.closed,
                            )
                            .await?
                        };
                        sailry_protocol::Output::ComputerPermissions(permissions)
                    }
                    sailry_protocol::Command::InspectHost => {
                        sailry_protocol::Output::HostInfo(self.host.inspect(self.node).await?)
                    }
                    sailry_protocol::Command::ListShells => {
                        let environment = self.environment.clone();
                        sailry_protocol::Output::Shells(
                            tokio::task::spawn_blocking(move || {
                                crate::terminal::shells::list(&environment)
                            })
                            .await
                            .map_err(|_| unavailable())?,
                        )
                    }
                    _ => sailry_protocol::Output::HostMetrics(self.host.sample(self.node).await?),
                };
                if !self.accepting.load(Ordering::Acquire) {
                    return Err(unavailable());
                }
                let (send, completion) = oneshot::channel();
                let _ = send.send(Ok(output));
                return Ok(Admission {
                    receipt: sailry_protocol::Receipt {
                        id: request.id,
                        durable: false,
                    },
                    completion,
                });
            }
            let (reply, response) = oneshot::channel();
            let knowledge_change = super::knowledge::runtime::changes(&request.command);
            let knowledge_invalidation = super::knowledge::runtime::invalidates(&request.command);
            let python_change = matches!(
                request.command,
                sailry_protocol::Command::SavePythonSettings(_)
            );
            let storage = self.storage.get().ok_or_else(unavailable)?;
            let order = if let Some(session) = super::routing::session(&request.command) {
                Some(storage.order(session).await)
            } else {
                None
            };
            let access = crate::files::Access::request(&request, internal);
            if super::routing::lifecycle(&request.command) {
                self.sender
                    .send(Job::Request {
                        access,
                        caller,
                        request: Box::new(request),
                        reply,
                    })
                    .await
                    .map_err(|_| unavailable())?;
            } else {
                let ingress = self.clone();
                self.operations
                    .spawn_child(&sailry_link::CancellationToken::new(), async move {
                        if let Some(work) = ingress
                            .persist_request(access, caller, Box::new(request), reply, order)
                            .await
                        {
                            ingress.handoff(work).await;
                        }
                    })
                    .await
                    .map_err(super::database::storage_error)?;
            }
            let result = response.await.map_err(|_| unavailable())?;
            self.dispatch.wake.notify_one();
            result.map(|admission| {
                if python_change {
                    super::python::observe(self, admission)
                } else if knowledge_change {
                    super::knowledge::runtime::observe(self, admission, knowledge_invalidation)
                } else {
                    admission
                }
            })
        })
    }
}
// Construct only the selected handler's future on its own poll boundary.
#[inline(never)]
fn defer<'a, F>(factory: impl FnOnce() -> F + Send + 'a) -> Pending<'a, Result<Admission, Fault>>
where
    F: std::future::Future<Output = Result<Admission, Fault>> + Send + 'a,
{
    Box::pin(async move { factory().await })
}
