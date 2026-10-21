//! Request routing shares authority checks; only internal callbacks await Store capacity.
use super::{Ingress, Job, unavailable};
use sailry_link::{Admission, Pending};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request};
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::{mpsc, oneshot};

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

    pub(super) async fn check_request(
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

    async fn recorded(
        &self,
        caller: NodeId,
        request: &Request,
        internal: bool,
    ) -> Result<bool, Fault> {
        let body = super::database::encode(request)?;
        let digest = blake3::hash(&body).to_hex().to_string();
        let inspection = Request::new(
            self.node,
            sailry_protocol::Command::InspectRequest {
                id: request.id,
                digest,
            },
        );
        let admission = self.dispatch_request(caller, inspection, internal).await?;
        match admission.completion.await.map_err(|_| unavailable())?? {
            sailry_protocol::Output::RequestOutcome { id, outcome } if id == request.id => Ok(
                !matches!(outcome, sailry_protocol::RequestOutcome::NotAdmitted),
            ),
            _ => Err(Fault::new(
                ErrorCode::Internal,
                "request outcome response expected",
            )),
        }
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
            if let sailry_protocol::Command::StartCodeServer { startup } = &request.command {
                if request.target != self.node || request.version != sailry_protocol::VERSION {
                    return Err(Fault::new(
                        ErrorCode::WrongTarget,
                        "Node or protocol version mismatch",
                    ));
                }
                // Restore the canonical receipt before inspecting mutable discovery or ownership.
                if !self.recorded(caller, &request, internal).await? {
                    self.check_request(caller, request.clone(), internal)
                        .await?;
                    let root = self.worktree_root_with(startup.worktree, internal).await?;
                    self.code.validate(startup, &root).await?;
                }
            }
            if let sailry_protocol::Command::InstallCodeServer { install } = &request.command {
                if request.target != self.node || request.version != sailry_protocol::VERSION {
                    return Err(Fault::new(
                        ErrorCode::WrongTarget,
                        "Node or protocol version mismatch",
                    ));
                }
                if !self.recorded(caller, &request, internal).await? {
                    self.check_request(caller, request.clone(), internal)
                        .await?;
                    self.code.validate_install(install)?;
                }
            }
            if request.plugin.is_some() && !request.command.durable() {
                self.check_request(caller, request.clone(), internal)
                    .await?;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ReadInstallations { .. }
            ) {
                return self.read_installations(caller, request);
            }
            if matches!(
                request.command,
                sailry_protocol::Command::DismissInstallation { .. }
            ) {
                return self.dismiss_installation(caller, request);
            }
            if matches!(request.command, sailry_protocol::Command::ListBackups) {
                return self.read_backup(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ReadArtifact { .. }
            ) {
                return self.read_artifact(caller, request, internal).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::FetchWeb(_)
                    | sailry_protocol::Command::CancelWebFetch { .. }
            ) {
                return self.fetch_web(caller, request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::SearchWeb(_)
                    | sailry_protocol::Command::CancelWebSearch { .. }
                    | sailry_protocol::Command::TestSearchDriver { .. }
            ) {
                return self.search_web(caller, request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ListChannelPresets { .. }
            ) {
                return self.list_channel_presets(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::SearchModels(_)
                    | sailry_protocol::Command::ReadModelVersions { .. }
                    | sailry_protocol::Command::ResolveModel { .. }
                    | sailry_protocol::Command::RefreshLocalModel { .. }
            ) {
                return self.model_catalog(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::SearchPluginValues(_)
            ) && self.knowledge.get().is_some()
            {
                return self.read_storage_search(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::QueryKnowledge(_)
                    | sailry_protocol::Command::ReadKnowledgeCitation(_)
            ) {
                return self.read_knowledge(request).await;
            }
            if matches!(request.command, sailry_protocol::Command::Decide(_)) {
                return self.decide(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ReadLocalModels { .. }
                    | sailry_protocol::Command::CancelLocalModel { .. }
            ) {
                return self.read_local_models(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ReadCode { .. }
                    | sailry_protocol::Command::ListCodeServers { .. }
                    | sailry_protocol::Command::ListCodePackages { .. }
                    | sailry_protocol::Command::ReadCodeCatalog
                    | sailry_protocol::Command::ListCodeInstallations
                    | sailry_protocol::Command::CancelCodeInstall { .. }
                    | sailry_protocol::Command::CancelCodeQuery { .. }
                    | sailry_protocol::Command::ReadCodeRules
            ) {
                return self.read_code(caller, request).await;
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
                    | sailry_protocol::Command::CheckPluginUpdates { .. }
            ) {
                return self.check_plugin_update(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::SearchPluginCatalog { .. }
                    | sailry_protocol::Command::SearchSkillCatalog { .. }
                    | sailry_protocol::Command::ReadCatalogPlugin { .. }
                    | sailry_protocol::Command::ReadCatalogPluginInfo { .. }
                    | sailry_protocol::Command::InspectPluginSource { .. }
            ) {
                return self.plugin_catalog(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ReadSkillDocument { .. }
                    | sailry_protocol::Command::DiscoverSkills { .. }
                    | sailry_protocol::Command::DiscoverRegistrySkill { .. }
            ) {
                return self.discover_skills(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::RequestProviderHttp { .. }
            ) {
                return self.request_provider_http(caller, request).await;
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
                    | sailry_protocol::Command::DownloadBackup { .. }
                    | sailry_protocol::Command::UploadBackup { .. }
                    | sailry_protocol::Command::UploadPlugin(_)
                    | sailry_protocol::Command::DiscoverSkillUpload { .. }
                    | sailry_protocol::Command::DiscoverLocalSkills { .. }
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
                    | sailry_protocol::Command::ReadFileLines { .. }
                    | sailry_protocol::Command::FindFiles { .. }
                    | sailry_protocol::Command::ReadOffice { .. }
                    | sailry_protocol::Command::ReadNotebook { .. }
                    | sailry_protocol::Command::SearchFiles { .. }
                    | sailry_protocol::Command::InspectGit { .. }
                    | sailry_protocol::Command::ReadGitStash { .. }
                    | sailry_protocol::Command::ReadGitOutput { .. }
                    | sailry_protocol::Command::ListGitRemoteTags { .. }
                    | sailry_protocol::Command::ListGitBranches { .. }
                    | sailry_protocol::Command::ResolveGitRevision { .. }
                    | sailry_protocol::Command::ListWorktrees { .. }
                    | sailry_protocol::Command::ReadGitLog { .. }
                    | sailry_protocol::Command::ReadGitShow { .. }
                    | sailry_protocol::Command::ReadGitBlame { .. }
                    | sailry_protocol::Command::ListGitChanges { .. }
                    | sailry_protocol::Command::ReadGitChange { .. }
                    | sailry_protocol::Command::ReadGitContext { .. }
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
            let job = Job::Request {
                access: crate::files::Access::request(&request, internal),
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
