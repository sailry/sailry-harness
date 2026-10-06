//! Request routing shares authority checks; only internal callbacks await Store capacity.
use super::{Ingress, Job, unavailable};
use sailry_link::{Admission, Pending};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request};
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::{mpsc, oneshot};

pub(super) fn release_removed(
    computer: Arc<crate::computer::Desktop>,
    runtime: &tokio::runtime::Handle,
    reply: oneshot::Sender<Result<Admission, Fault>>,
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
                        for session in sessions {
                            computer.finish(*session).await;
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
    pub(crate) async fn dispatch_internal(&self, request: Request) -> Result<Admission, Fault> {
        self.dispatch_request(self.node, request, true).await
    }

    pub(super) async fn send_internal(&self, job: Job) -> Result<(), Fault> {
        tokio::select! {
            biased;
            _ = self.closed.cancelled() => Err(unavailable()),
            result = self.sender.send(job) => result.map_err(|_| unavailable()),
        }
    }

    async fn check_request(
        &self,
        caller: NodeId,
        request: Request,
        internal: bool,
    ) -> Result<(), Fault> {
        if !internal {
            return self.check_plugin(caller, request).await;
        }
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::CheckPlugin {
            caller,
            request: Box::new(request),
            reply,
        })
        .await?;
        response.await.map_err(|_| unavailable())?
    }

    pub(super) fn dispatch_request(
        &self,
        caller: NodeId,
        request: Request,
        internal: bool,
    ) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if !self.accepting.load(Ordering::Acquire) {
                return Err(unavailable());
            }
            if request.plugin.is_some() && !request.command.durable() {
                self.check_request(caller, request.clone(), internal)
                    .await?;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ListCommands { .. }
                    | sailry_protocol::Command::ReadCommand { .. }
            ) {
                return self.read_command(request);
            }
            if let sailry_protocol::Command::CompleteBrowser { id, result } = &request.command {
                if request.target != self.node || request.version != sailry_protocol::VERSION {
                    return Err(Fault::new(
                        ErrorCode::WrongTarget,
                        "Node or protocol version mismatch",
                    ));
                }
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
            if matches!(
                request.command,
                sailry_protocol::Command::OpenPort { .. }
                    | sailry_protocol::Command::CancelPort { .. }
            ) {
                return self.port(caller, request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ReadPluginMcp { .. }
            ) {
                return self.read_plugin_mcp(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ReadPluginView { .. }
            ) {
                return self.read_plugin_view(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::CheckPluginUpdate { .. }
            ) {
                return self.check_plugin_update(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::SearchPluginCatalog { .. }
                    | sailry_protocol::Command::ReadCatalogPlugin { .. }
                    | sailry_protocol::Command::ReadCatalogPluginInfo { .. }
                    | sailry_protocol::Command::InspectPluginSource { .. }
            ) {
                return self.plugin_catalog(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::DiscoverSkills { .. }
            ) {
                return self.discover_skills(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::DiscoverModels(_)
                    | sailry_protocol::Command::ValidateProvider { .. }
            ) {
                return self.discover_models(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::RefreshModelCatalog
            ) {
                return self.refresh_catalog(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::InspectTerminal { .. }
                    | sailry_protocol::Command::ClaimTerminal { .. }
                    | sailry_protocol::Command::InputTerminal { .. }
                    | sailry_protocol::Command::ResizeTerminal { .. }
                    | sailry_protocol::Command::SetTerminalAppearance { .. }
            ) {
                return self.terminal(caller, request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::DownloadFile { .. }
                    | sailry_protocol::Command::PreviewOffice { .. }
                    | sailry_protocol::Command::UploadPlugin(_)
                    | sailry_protocol::Command::InspectPluginUpload { .. }
                    | sailry_protocol::Command::UploadFile(_)
                    | sailry_protocol::Command::StageSshUpload(_)
                    | sailry_protocol::Command::UploadAttachment(_)
                    | sailry_protocol::Command::DownloadAttachment { .. }
                    | sailry_protocol::Command::DownloadImage { .. }
                    | sailry_protocol::Command::CancelFileTransfer { .. }
            ) {
                return self.transfer(caller, request, internal).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ListDirectory { .. }
                    | sailry_protocol::Command::BrowseFiles { .. }
                    | sailry_protocol::Command::ListTerminalTools { .. }
                    | sailry_protocol::Command::ReadFile { .. }
                    | sailry_protocol::Command::ReadOffice { .. }
                    | sailry_protocol::Command::SearchFiles { .. }
                    | sailry_protocol::Command::InspectGit { .. }
                    | sailry_protocol::Command::ReadGitStash { .. }
                    | sailry_protocol::Command::ReadGitOutput { .. }
                    | sailry_protocol::Command::ListGitRemoteTags { .. }
                    | sailry_protocol::Command::ListGitBranches { .. }
                    | sailry_protocol::Command::ResolveGitRevision { .. }
                    | sailry_protocol::Command::ListWorktrees { .. }
                    | sailry_protocol::Command::ReadGitLog { .. }
                    | sailry_protocol::Command::ReadGitCommit { .. }
                    | sailry_protocol::Command::ReadGitDiff { .. }
            ) {
                return self.inspect_worktree(request, internal).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::InspectHost
                    | sailry_protocol::Command::ReadHostMetrics
                    | sailry_protocol::Command::ReadComputerPermissions
                    | sailry_protocol::Command::RequestComputerPermission { .. }
                    | sailry_protocol::Command::ListShells
            ) {
                if request.target != self.node || request.version != sailry_protocol::VERSION {
                    return Err(Fault::new(
                        sailry_protocol::ErrorCode::WrongTarget,
                        "Node or protocol version mismatch",
                    ));
                }
                if request.plugin.is_some() {
                    self.check_request(caller, request.clone(), internal)
                        .await?;
                }
                let output = match request.command {
                    sailry_protocol::Command::ReadComputerPermissions => {
                        let node = self.node;
                        sailry_protocol::Output::ComputerPermissions(
                            tokio::task::spawn_blocking(move || {
                                crate::computer::permissions::read(node, caller)
                            })
                            .await
                            .map_err(|_| unavailable())?,
                        )
                    }
                    sailry_protocol::Command::RequestComputerPermission { permission } => {
                        let node = self.node;
                        sailry_protocol::Output::ComputerPermissions(
                            tokio::task::spawn_blocking(move || {
                                crate::computer::permissions::request(node, caller, permission)
                            })
                            .await
                            .map_err(|_| unavailable())??,
                        )
                    }
                    sailry_protocol::Command::InspectHost => {
                        sailry_protocol::Output::HostInfo(self.host.inspect(self.node).await?)
                    }
                    sailry_protocol::Command::ListShells => sailry_protocol::Output::Shells(
                        tokio::task::spawn_blocking(crate::terminal::shells::list)
                            .await
                            .map_err(|_| unavailable())?,
                    ),
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
            let job = Job::Request {
                caller,
                request: Box::new(request),
                reply,
            };
            if internal {
                self.send_internal(job).await?;
            } else {
                self.sender.try_send(job).map_err(|error| match error {
                    mpsc::error::TrySendError::Full(_) => {
                        Fault::new(ErrorCode::Busy, "Node request queue is full")
                    }
                    mpsc::error::TrySendError::Closed(_) => unavailable(),
                })?;
            }
            let result = response.await.map_err(|_| unavailable())?;
            self.dispatch.wake.notify_one();
            result
        })
    }
}
