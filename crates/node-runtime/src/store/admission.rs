//! Request routing shares authority checks; only internal callbacks await Store capacity.
use super::{Ingress, Job, unavailable};
use sailry_link::{Admission, Pending};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request};
use std::sync::{Arc, atomic::Ordering};
use tokio::sync::{mpsc, oneshot};

pub(super) fn release_removed(
    computer: Arc<crate::computer::Desktop>,
    commands: Arc<crate::process::background::Commands>,
    profile: Option<std::path::PathBuf>,
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
                        let mut owners = match cleanup.await {
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
                            computer.finish(*session).await;
                            commands.close_session(*session).await;
                            if let Some(profile) = &profile {
                                let profile = profile.clone();
                                let session = *session;
                                let artifacts = owners.contains(&session);
                                owners.retain(|owner| *owner != session);
                                match tokio::task::spawn_blocking(move || {
                                    remove_files(&profile, session, artifacts)
                                })
                                .await
                                {
                                    Ok(Ok(())) => {}
                                    Ok(Err(error)) => eprintln!(
                                        "removed session directory cleanup failed: {error}"
                                    ),
                                    Err(error) => {
                                        eprintln!("removed session cleanup worker failed: {error}")
                                    }
                                }
                            }
                        }
                        if let Some(profile) = &profile {
                            let profile = profile.clone();
                            match tokio::task::spawn_blocking(move || {
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

fn remove_files(
    profile: &std::path::Path,
    session: sailry_protocol::SessionId,
    remove_artifacts: bool,
) -> Result<(), Fault> {
    let workspace = crate::python::remove_session(profile, session);
    let artifacts = if remove_artifacts {
        crate::files::artifacts::remove(profile, session)
    } else {
        Ok(())
    };
    workspace.and(artifacts)
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
                sailry_protocol::Command::SearchModels(_)
                    | sailry_protocol::Command::ReadModelVersions { .. }
                    | sailry_protocol::Command::ResolveModel { .. }
                    | sailry_protocol::Command::RefreshOcrModel { .. }
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
                    | sailry_protocol::Command::ReadKnowledgeModels
                    | sailry_protocol::Command::CancelKnowledgeModel { .. }
            ) {
                return self.read_knowledge(request).await;
            }
            if matches!(
                request.command,
                sailry_protocol::Command::ReadOcrModels
                    | sailry_protocol::Command::CancelOcrModel { .. }
            ) {
                return self.read_ocr(request).await;
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
                sailry_protocol::Command::ReadGithubLogin { .. }
            ) {
                return self.read_github_login(request).await;
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
            let ocr_change = matches!(
                request.command,
                sailry_protocol::Command::SaveOcrSettings(_)
                    | sailry_protocol::Command::RegisterOcrModel { .. }
            );
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
            result.map(|admission| {
                if ocr_change {
                    super::ocr::observe(self, admission)
                } else if python_change {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_artifacts_after_workspace_failure() {
        let profile = tempfile::tempdir().unwrap();
        let root = profile.path().canonicalize().unwrap();
        let session = sailry_protocol::SessionId::new();
        let artifact = sailry_protocol::ArtifactId::new();
        crate::files::artifacts::write(&root, session, artifact, 1, "saved").unwrap();
        std::fs::create_dir(root.join("workspaces")).unwrap();
        std::fs::write(root.join("workspaces/sessions"), "obstruction").unwrap();
        let expected = crate::python::remove_session(&root, session).unwrap_err();
        let error = remove_files(&root, session, true).unwrap_err();
        assert_eq!(error, expected);
        assert!(!root.join("artifacts").join(session.to_string()).exists());
        assert_eq!(
            std::fs::read_to_string(root.join("workspaces/sessions")).unwrap(),
            "obstruction"
        );
    }
}
