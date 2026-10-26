use std::path::Path;

use sailry_link::{Admission, Response};
use sailry_protocol::*;
use tokio::sync::{broadcast, oneshot};
use turso::{Connection, params};

use super::{commands, external::Work, mutations};
use crate::Error;

pub(in crate::store) mod conflict;
mod context;
mod driver;
mod relocation;
mod removal;
pub(in crate::store) use context::{Facts, Storage, WRITE_CONCURRENCY};
pub(crate) use driver::{Exclusive, Handle, Lease, connect, open as engine};
pub(in crate::store) mod counters;
pub(super) mod schema;
mod startup;
pub(super) mod validation;

const APPLICATION_ID: i64 = 0x5341_494c;

pub(super) struct Database {
    pub(super) connection: Lease,
    pub(super) handle: std::sync::Arc<Handle>,
    pub(super) path: std::path::PathBuf,
    facts: std::sync::Arc<Facts>,
}

impl Database {
    pub(super) async fn models(
        &mut self,
        registry: std::sync::Arc<crate::models::registry::Registry>,
    ) -> Result<(), Fault> {
        let catalog = super::local_models::read(&self.connection).await?;
        let prepared = registry.prepare(catalog.settings, catalog.definitions)?;
        registry.apply(prepared);
        std::sync::Arc::get_mut(&mut self.facts)
            .ok_or_else(|| storage_error("model registry is already shared"))?
            .local_models = Some(registry);
        Ok(())
    }

    pub(super) async fn worktree_root(&self, id: WorktreeId) -> Result<std::path::PathBuf, Fault> {
        self.check_removing(id)?;
        let row = self
            .connection
            .prepare("SELECT path FROM worktrees WHERE id=?1")
            .await
            .map_err(storage_error)?
            .query_row([id.to_string()])
            .await;
        let root = match row {
            Ok(row) => std::path::PathBuf::from(row.get::<String>(0).map_err(storage_error)?),
            Err(turso::Error::QueryReturnedNoRows) => {
                return Err(Fault::new(
                    ErrorCode::NotFound,
                    "worktree does not exist on this Node",
                ));
            }
            Err(error) => return Err(storage_error(error)),
        };
        super::worktrees::present(&self.connection, id).await?;
        super::worktrees::materialize_session(&self.connection, self.profile.as_deref(), id, &root)
            .await?;
        Ok(root)
    }

    pub(super) async fn resolve_credential(
        &self,
        reference: &CredentialRef,
        provider: ProviderId,
        authentication: Authentication,
    ) -> Result<Secret, Fault> {
        self.credential(reference, provider, authentication)
            .await
            .map(|(_, secret)| secret)
    }

    pub(super) async fn credential(
        &self,
        reference: &CredentialRef,
        provider: ProviderId,
        authentication: Authentication,
    ) -> Result<(Credential, Secret), Fault> {
        if reference.node != self.node {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "credential belongs to another execution Node",
            ));
        }
        super::providers::authentication::resolve(
            &self.connection,
            reference.id,
            provider,
            authentication,
        )
        .await
    }

    pub(super) async fn addresses(&self) -> Result<Vec<sailry_link::EndpointAddr>, Fault> {
        let mut rows = self
            .connection
            .query("SELECT address FROM link_addresses ORDER BY identity", ())
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let body: String = row.get(0).map_err(storage_error)?;
            values.push(
                serde_json::from_str(&body).map_err(|_| storage_error("invalid peer address"))?,
            );
        }
        Ok(values)
    }

    pub(super) async fn remember(&self, address: sailry_link::EndpointAddr) -> Result<(), Fault> {
        let body = serde_json::to_string(&address).map_err(storage_error)?;
        if body.len() > 8192 || address.addrs.len() > 16 {
            return Err(Fault::invalid("peer address exceeds limits"));
        }
        self.connection.execute("INSERT INTO link_addresses(identity,address) VALUES(?1,?2) ON CONFLICT(identity) DO UPDATE SET address=excluded.address", params![&address.id.as_bytes()[..], body]).await.map_err(storage_error)?;
        Ok(())
    }

    pub(super) async fn peers(&self) -> Result<Vec<NodeId>, Fault> {
        let mut rows = self
            .connection
            .query("SELECT identity FROM link_peers ORDER BY identity", ())
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let bytes: [u8; 32] = row.get(0).map_err(storage_error)?;
            values.push(NodeId(bytes));
        }
        Ok(values)
    }

    pub(super) async fn set_trust(&self, peer: NodeId, trusted: bool) -> Result<(), Fault> {
        if trusted {
            let count: i64 = self
                .connection
                .prepare("SELECT count(*) FROM link_peers")
                .await
                .map_err(storage_error)?
                .query_row(())
                .await
                .map_err(storage_error)?
                .get(0)
                .map_err(storage_error)?;
            if count >= 128 && !self.peers().await?.contains(&peer) {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "paired device capacity exhausted",
                ));
            }
            self.connection
                .execute(
                    "INSERT OR IGNORE INTO link_peers(identity) VALUES(?1)",
                    [&peer.0[..]],
                )
                .await
                .map_err(storage_error)?;
        } else {
            self.connection
                .execute("DELETE FROM link_peers WHERE identity=?1", [&peer.0[..]])
                .await
                .map_err(storage_error)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) async fn open(
        path: &Path,
        node: NodeId,
        profile: Option<std::path::PathBuf>,
    ) -> Result<Self, Error> {
        Self::open_with_plugins(path, node, profile, None).await
    }

    pub(super) fn open_with_plugins<'a>(
        path: &'a Path,
        node: NodeId,
        profile: Option<std::path::PathBuf>,
        plugins: Option<&'a crate::plugins::Host>,
    ) -> sailry_link::Pending<'a, Result<Self, Error>> {
        Box::pin(async move {
            let (database, connection) = startup::open(path, node, plugins).await?;
            drop(connection);
            let handle = std::sync::Arc::new(Handle::new(path.to_owned(), database));
            let connection = handle.lease(false).await?;
            let writes = handle.writes.clone();
            Ok(Self::context(
                handle,
                std::sync::Arc::new(Facts::new(node, profile, writes)),
                connection,
            ))
        })
    }

    pub(super) async fn close(self) -> Result<(), Error> {
        self.into_storage().close().await
    }

    pub(super) fn read<'a>(
        &'a self,
        caller: NodeId,
        request: &'a Request,
    ) -> sailry_link::Pending<'a, Response> {
        Box::pin(async move {
            let history = match &request.command {
                Command::ReadConversation { session, .. } | Command::ReadTurn { session, .. } => {
                    Some(*session)
                }
                _ => None,
            };
            let order = match history {
                Some(_) => Some(self.publication.clone().lock().await),
                None => None,
            };
            let transaction = self
                .connection
                .unchecked_transaction()
                .await
                .map_err(storage_error)?;
            let sequence = if let Some(session) = history {
                // Pin the SQL snapshot before capturing the matching feed watermark.
                transaction
                    .prepare("SELECT coalesce(max(cursor),0) FROM events")
                    .await
                    .map_err(storage_error)?
                    .query_row(())
                    .await
                    .map_err(storage_error)?;
                Some(self.feeds.sequence(session))
            } else {
                None
            };
            drop(order);
            let result = async {
                self.check_plugin(caller, request).await?;
                self.check_removal_scope(&request.command).await?;
                let (mut output, event) = commands::execute(
                    &transaction,
                    self.node,
                    caller,
                    self.profile.as_deref(),
                    request,
                )
                .await?;
                if event.is_some() {
                    return Err(storage_error("a read produced a mutation event"));
                }
                match (&mut output, sequence) {
                    (Output::Conversation(history), Some(sequence)) => history.sequence = sequence,
                    (Output::TurnHistory(history), Some(sequence)) => history.sequence = sequence,
                    _ => {}
                }
                Ok(output)
            }
            .await;
            match result {
                Ok(output) => {
                    crate::store::database::conflict::commit(transaction).await?;
                    Ok(output)
                }
                Err(error) => {
                    transaction.rollback().await.map_err(storage_error)?;
                    Err(error)
                }
            }
        })
    }

    #[cfg(test)]
    pub(super) async fn dispatch(
        &mut self,
        caller: NodeId,
        request: Request,
        reply: oneshot::Sender<Result<Admission, Fault>>,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Option<Work> {
        self.dispatch_with(
            caller,
            request,
            reply,
            events,
            crate::files::Access::Restricted,
        )
        .await
    }

    pub(super) fn dispatch_with<'a>(
        &'a mut self,
        caller: NodeId,
        request: Request,
        reply: oneshot::Sender<Result<Admission, Fault>>,
        events: &'a broadcast::Sender<EventEnvelope>,
        access: crate::files::Access,
    ) -> sailry_link::Pending<'a, Option<Work>> {
        Box::pin(async move {
            if request.target != self.node || request.version != VERSION {
                let _ = reply.send(Err(Fault::new(
                    ErrorCode::WrongTarget,
                    "Node or protocol version mismatch",
                )));
                return None;
            }
            let receipt = Receipt {
                id: request.id,
                durable: request.command.durable(),
            };
            let plugin_command = super::plugins::command::resolve(self, &request).await;
            if matches!(plugin_command, Ok(None)) && !super::routing::external(&request.command) {
                let output = match self.execute(caller, &request, events).await {
                    Ok(output) => output,
                    Err(error) => {
                        let _ = reply.send(Err(error));
                        return None;
                    }
                };
                let (completion, result) = oneshot::channel();
                let _ = completion.send(output);
                // Database-only admission and its outcome have committed together.
                let _ = reply.send(Ok(Admission {
                    receipt,
                    completion: result,
                }));
                return None;
            }
            let previous = match self.admit(caller, &request).await {
                Ok(previous) => previous,
                Err(error) => {
                    let _ = reply.send(Err(error));
                    return None;
                }
            };
            let (completion, result) = oneshot::channel();
            // admit() has committed before this receipt can reach either transport.
            let _ = reply.send(Ok(Admission {
                receipt,
                completion: result,
            }));
            let output = if let Some(previous) = previous {
                previous
            } else if request.command.durable()
                && let Err(error) = self.check_plugin(caller, &request).await
            {
                self.finish_external(caller, &request, Err(error), events)
                    .await
            } else if let Err(error) = self.check_removal_scope(&request.command).await {
                self.finish_external(caller, &request, Err(error), events)
                    .await
            } else if let Err(error) = plugin_command {
                self.finish_external(caller, &request, Err(error), events)
                    .await
            } else if let Ok(Some(child)) = plugin_command {
                return Some(Work::PluginExecution(Box::new(
                    super::plugins::execution::Execution {
                        caller,
                        request,
                        input: super::plugins::execution::Input::Command(child),
                        reply: completion,
                    },
                )));
            } else if matches!(
                request.command,
                Command::ReadNetworkSettings | Command::SaveNetworkSettings { .. }
            ) {
                match super::network::prepare(self, caller, request, completion).await {
                    Ok(execution) => return Some(Work::Network(execution)),
                    Err(failure) => {
                        let (request, completion, error) = *failure;
                        let result = self
                            .finish_external(caller, &request, Err(error), events)
                            .await;
                        let _ = completion.send(result);
                        return None;
                    }
                }
            } else if matches!(
                request.command,
                Command::BeginProviderLogin { .. }
                    | Command::CompleteProviderLogin { .. }
                    | Command::CancelProviderLogin { .. }
                    | Command::StartConnectorLogin { .. }
                    | Command::SubmitConnectorInput { .. }
                    | Command::CancelConnectorLogin { .. }
                    | Command::ProbeConnection { .. }
                    | Command::InspectConnectorCli { .. }
            ) {
                return Some(Work::Login(super::login::Operation {
                    caller,
                    request,
                    reply: completion,
                }));
            } else if matches!(
                request.command,
                Command::InstallHost { .. }
                    | Command::UpdateSshHost { .. }
                    | Command::ReadHostInstall { .. }
                    | Command::CheckSsh { .. }
                    | Command::OpenSshTerminal { .. }
                    | Command::CloseSshTerminal { .. }
                    | Command::RunSsh { .. }
                    | Command::TransferSsh { .. }
                    | Command::BrowseSshDirectory { .. }
                    | Command::DownloadSshFile { .. }
                    | Command::FinishSshUpload { .. }
                    | Command::ModifySshFile { .. }
                    | Command::CancelSsh { .. }
            ) {
                let prepared = if matches!(
                    request.command,
                    Command::CancelSsh { .. }
                        | Command::CloseSshTerminal { .. }
                        | Command::ReadHostInstall { .. }
                ) {
                    Ok(None)
                } else {
                    super::ssh::prepare(self, &request.command).await.map(Some)
                };
                match prepared {
                    Ok(operation) => {
                        return Some(Work::Ssh(super::ssh::worker::Execution {
                            caller,
                            request,
                            operation,
                            reply: completion,
                        }));
                    }
                    Err(error) => {
                        self.finish_external(caller, &request, Err(error), events)
                            .await
                    }
                }
            } else if matches!(
                request.command,
                Command::BrowseDatabase { .. }
                    | Command::CheckDatabase { .. }
                    | Command::TestDatabase { .. }
                    | Command::QueryDatabase { .. }
                    | Command::CancelDatabase { .. }
            ) {
                let prepared = if matches!(request.command, Command::CancelDatabase { .. }) {
                    Ok(None)
                } else {
                    super::databases::prepare(self, &request.command)
                        .await
                        .map(Some)
                };
                match prepared {
                    Ok(operation) => {
                        return Some(Work::Database(super::databases::worker::Execution {
                            caller,
                            request,
                            operation,
                            reply: completion,
                        }));
                    }
                    Err(error) => {
                        self.finish_external(caller, &request, Err(error), events)
                            .await
                    }
                }
            } else if matches!(
                request.command,
                Command::GeneratePluginText { .. }
                    | Command::RequestPluginHttp(_)
                    | Command::ExecuteProviderHttp { .. }
                    | Command::CallPlugin { .. }
                    | Command::ReadPluginCallback { .. }
                    | Command::CancelPluginCall { .. }
            ) {
                match super::plugins::execution::prepare(self, caller, &request).await {
                    Ok(input) => {
                        return Some(Work::PluginExecution(Box::new(
                            super::plugins::execution::Execution {
                                caller,
                                request,
                                input,
                                reply: completion,
                            },
                        )));
                    }
                    Err(error) => {
                        self.finish_external(caller, &request, Err(error), events)
                            .await
                    }
                }
            } else if matches!(
                request.command,
                Command::CloseTerminal { .. }
                    | Command::CheckPluginMcp { .. }
                    | Command::RestoreBackupFile { .. }
                    | Command::ExportBackup { .. }
                    | Command::RestoreBackup { .. }
                    | Command::InstallLocalModel { .. }
                    | Command::DeleteLocalModel { .. }
                    | Command::UseExternalBrowser { .. }
                    | Command::UseBrowser { .. }
                    | Command::UseMedia { .. }
            ) {
                match super::resources::prepare(self, &request).await {
                    Ok(()) => {
                        return Some(Work::Resources(super::resources::Execution {
                            caller,
                            request,
                            reply: completion,
                        }));
                    }
                    Err(error) => {
                        self.finish_external(caller, &request, Err(error), events)
                            .await
                    }
                }
            } else if matches!(request.command, Command::RunCommand { .. }) {
                match match super::processes::prepare(self, &request).await {
                    Ok(value) => {
                        (async |launch: super::processes::Prepared| {
                            let publication = self.publication.clone();
                            let transaction = self
                                .connection
                                .unchecked_transaction()
                                .await
                                .map_err(storage_error)?;
                            let task =
                                super::agent::background::command_started(&transaction, &request)
                                    .await?;
                            let _publication = publication.lock().await;
                            crate::store::database::conflict::commit(transaction).await?;
                            if let Some(task) = task {
                                super::agent::background::publish(self, [task]).await;
                            }
                            Ok(launch)
                        })(value)
                        .await
                    }
                    Err(error) => Err(error),
                } {
                    Ok(launch) => {
                        return Some(Work::Process(super::processes::Execution {
                            caller,
                            request,
                            launch,
                            reply: completion,
                        }));
                    }
                    Err(error) => {
                        self.finish_external(caller, &request, Err(error), events)
                            .await
                    }
                }
            } else if matches!(request.command, Command::CreateSessionAt { .. }) {
                match super::forwarding::prepare(self, caller, &request).await {
                    Ok(forwarded) => {
                        return Some(Work::Session(super::forwarding::Transfer {
                            caller,
                            request,
                            forwarded: Box::new(forwarded),
                            reply: completion,
                        }));
                    }
                    Err(error) => {
                        self.finish_external(caller, &request, Err(error), events)
                            .await
                    }
                }
            } else if let Some(target) = mutations::target(&request.command) {
                let roots = match target {
                    mutations::Target::NewProject => {
                        let Command::CreateProject(draft) = &request.command else {
                            unreachable!()
                        };
                        match super::projects::prepare(draft) {
                            Ok(value) => {
                                (async |path: std::path::PathBuf| {
                                    if matches!(
                                        draft.source,
                                        sailry_protocol::projects::Source::Clone { .. }
                                    ) {
                                        let parent = path.parent().unwrap().to_owned();
                                        let name =
                                            path.file_name().unwrap().to_str().unwrap().to_owned();
                                        self.begin_relocation(
                                            caller,
                                            request.id,
                                            &[(parent, name)],
                                            access,
                                        )
                                        .await?;
                                    }
                                    Ok((path.clone(), path))
                                })(value)
                                .await
                            }
                            Err(error) => Err(error),
                        }
                    }
                    mutations::Target::ManagedWorktree { project, source } => {
                        match match super::worktrees::select(
                            &self.connection,
                            project,
                            Some(source),
                        )
                        .await
                        {
                            Ok(value) => {
                                async {
                                    let source = value;
                                    self.worktree_root(source).await
                                }
                                .await
                            }
                            Err(error) => Err(error),
                        } {
                            Ok(value) => {
                                async {
                                    let source = value;
                                    super::worktrees::project_root(&self.connection, project)
                                        .await
                                        .map(|root| (source, crate::workspaces::worktrees(&root)))
                                }
                                .await
                            }
                            Err(error) => Err(error),
                        }
                    }
                    mutations::Target::Host => {
                        let validation = if let Command::ManageFiles(action) = &request.command {
                            match crate::files::browser::targets(action) {
                                Ok(value) => {
                                    async {
                                        let paths: Vec<(std::path::PathBuf, String)> = value;
                                        self.begin_relocation(caller, request.id, &paths, access)
                                            .await
                                    }
                                    .await
                                }
                                Err(error) => Err(error),
                            }
                        } else if let Command::RemoveProject { expected } = &request.command {
                            self.begin_project_removal(caller, &request, expected).await
                        } else {
                            Ok(())
                        };
                        validation.map(|()| (std::path::PathBuf::new(), std::path::PathBuf::new()))
                    }
                    mutations::Target::Plugin => {
                        Ok((std::path::PathBuf::new(), std::path::PathBuf::new()))
                    }
                    mutations::Target::Worktree(id) => self
                        .worktree_root(id)
                        .await
                        .map(|root| (root.clone(), root)),
                    mutations::Target::Project(id) => {
                        super::worktrees::project_root(&self.connection, id)
                            .await
                            .map(|root| (root.clone(), root))
                    }
                    mutations::Target::Worktrees { source, target } => {
                        match self.worktree_root(source).await {
                            Ok(value) => {
                                async {
                                    let source = value;
                                    self.worktree_root(target)
                                        .await
                                        .map(|target| (source, target))
                                }
                                .await
                            }
                            Err(error) => Err(error),
                        }
                    }
                };
                let roots = match roots {
                    Ok(value) => {
                        (async |(source, target)| {
                            let roots = mutations::Roots {
                                source,
                                target,
                                access,
                            };
                            if let Command::InstallPlugin {
                                name,
                                expected_revision,
                                ..
                            }
                            | Command::InstallPluginUpload {
                                name,
                                expected_revision,
                                ..
                            }
                            | Command::InstallPluginSource {
                                name,
                                expected_revision,
                                ..
                            }
                            | Command::InstallBundledPlugin {
                                name,
                                expected_revision,
                            }
                            | Command::InstallSkillUpload {
                                name,
                                expected_revision,
                                ..
                            }
                            | Command::InstallRegistrySkill {
                                name,
                                expected_revision,
                                ..
                            }
                            | Command::InstallSkill {
                                name,
                                expected_revision,
                                ..
                            }
                            | Command::InstallMcp {
                                name,
                                expected_revision,
                                ..
                            } = &request.command
                            {
                                super::plugins::check_install(
                                    &self.connection,
                                    name,
                                    *expected_revision,
                                )
                                .await?;
                            }
                            if let Command::SetPluginEnabled {
                                name,
                                expected_revision,
                                enabled: true,
                            } = &request.command
                            {
                                super::plugins::prepare_enable(
                                    &self.connection,
                                    name,
                                    *expected_revision,
                                )
                                .await?;
                            }
                            if let Command::RemoveWorktree { worktree, .. } = request.command {
                                self.begin_removal(caller, &request, worktree, &roots.target)
                                    .await?;
                            }
                            let mut paths = roots.entries(&request.command);
                            if let Command::ApplyTurnHunk(selection) = &request.command {
                                super::agent::checkpoints::hunks::prepare(
                                    self,
                                    selection,
                                    &roots.target,
                                )
                                .await?;
                            }
                            if let Command::RestoreFileCheckpoint {
                                session,
                                checkpoint,
                                worktree,
                            } = request.command
                            {
                                let content = super::agent::checkpoints::restore::prepare(
                                    self,
                                    session,
                                    checkpoint,
                                    worktree,
                                    &roots.target,
                                )
                                .await?;
                                paths.push((roots.target.clone(), content.file.path));
                            }
                            if !paths.is_empty() {
                                self.begin_relocation(caller, request.id, &paths, access)
                                    .await?;
                            }
                            Ok(roots)
                        })(value)
                        .await
                    }
                    Err(error) => Err(error),
                };
                match roots {
                    Ok(roots) => {
                        return Some(Work::Mutation(mutations::Mutation {
                            caller,
                            request,
                            roots,
                            reply: completion,
                        }));
                    }
                    Err(error) => {
                        self.finish_external(caller, &request, Err(error), events)
                            .await
                    }
                }
            } else {
                self.execute(caller, &request, events)
                    .await
                    .unwrap_or_else(Err)
            };
            let _ = completion.send(output);
            None
        })
    }

    async fn admit(
        &mut self,
        caller: NodeId,
        request: &Request,
    ) -> Result<Option<Response>, Fault> {
        if !request.command.durable() {
            return Ok(None);
        }
        let write = self.write().await?;
        for attempt in 0..=conflict::RETRIES {
            let result = async {
                let transaction = conflict::begin(&self.connection).await?;
                let previous = admit_in(&transaction, caller, request).await?;
                conflict::commit(transaction).await?;
                Ok(previous)
            }
            .await;
            if let Err(error) = conflict::rollback(&self.connection).await {
                self.connection.hold_write(write);
                return Err(error);
            }
            if !conflict::again(&result, attempt).await {
                return result;
            }
        }
        unreachable!("the final attempt returns its result")
    }

    fn execute<'a>(
        &'a mut self,
        caller: NodeId,
        request: &'a Request,
        events: &'a broadcast::Sender<EventEnvelope>,
    ) -> sailry_link::Pending<'a, Result<Response, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::ResolveApproval { .. }) {
                super::agent::approvals::prune(self, events).await?;
            }
            let external = super::routing::external(&request.command);
            let write = if request.command.durable() {
                Some(self.write().await?)
            } else {
                None
            };
            let attempts = if external { 0 } else { conflict::RETRIES };
            for attempt in 0..=attempts {
                let result = self.execute_once(caller, request, events).await;
                if let Err(error) = conflict::rollback(&self.connection).await {
                    if let Some(write) = write {
                        self.connection.hold_write(write);
                    }
                    return Err(error);
                }
                if external {
                    return result.map_err(|error| {
                        super::outcomes::failed(self.node, caller, request.id, &error);
                        Fault::new(ErrorCode::OutcomeUnknown,
                            "operation result could not be recorded; inspect the target before retrying")
                    });
                }
                if !conflict::again(&result, attempt).await {
                    return result;
                }
            }
            unreachable!("the final attempt returns its result")
        })
    }

    fn execute_once<'a>(
        &'a mut self,
        caller: NodeId,
        request: &'a Request,
        events: &'a broadcast::Sender<EventEnvelope>,
    ) -> sailry_link::Pending<'a, Result<Response, Fault>> {
        Box::pin(async move {
            let approval = matches!(request.command, Command::ResolveApproval { .. });
            let publication = self.publication.clone();
            let schema =
                super::plugins::storage::search::creates(&self.connection, request).await?;
            let transaction = conflict::begin_with(&self.connection, schema).await?;
            if !super::routing::external(&request.command)
                && let Some(previous) = admit_in(&transaction, caller, request).await?
            {
                conflict::rollback(&transaction).await?;
                return Ok(previous);
            }
            // Commands validate before mutation. A savepoint also rolls back partial business errors.
            transaction
                .execute_batch("SAVEPOINT command")
                .await
                .map_err(storage_error)?;
            let mut review_changes = super::agent::review::Changes::new();
            let mut model_configuration = None;
            let scope = async {
                self.check_plugin(caller, request).await?;
                self.check_removal_scope(&request.command).await
            }
            .await;
            let outcome = if let Err(error) = scope {
                Err(error)
            } else if matches!(request.command, Command::PluginTransaction { .. }) {
                super::plugins::transaction::execute(
                    &transaction,
                    self.node,
                    caller,
                    self.profile.as_deref(),
                    request,
                )
                .await
            } else if matches!(
                request.command,
                Command::ArchiveSessions { .. } | Command::RemoveSessions { .. }
            ) {
                super::sessions::batch::execute(&transaction, request).await
            } else {
                commands::execute(
                    &transaction,
                    self.node,
                    caller,
                    self.profile.as_deref(),
                    request,
                )
                .await
                .map(|(output, event)| (output, event.into_iter().collect()))
            };
            let outcome = match outcome {
                Ok(value) => {
                    (async |(output, mut changes): (Output, Vec<Event>)| {
                        if self.profile.is_some()
                            && changes
                                .iter()
                                .any(|event| matches!(event, Event::PluginChanged(_)))
                            && super::knowledge::semantic::startup(
                                &transaction,
                                &sailry_link::CancellationToken::new(),
                            )
                            .await?
                        {
                            changes.push(Event::KnowledgeCollectionsChanged);
                        }
                        if self.profile.is_some()
                            && super::knowledge::runtime::storage::changes(&request.command)
                            && super::knowledge::semantic::changed(
                                &transaction,
                                request,
                                &sailry_link::CancellationToken::new(),
                            )
                            .await?
                        {
                            changes.push(Event::KnowledgeCollectionsChanged);
                        }
                        if super::providers::quota::affected(&request.command) {
                            changes.extend(
                                super::providers::quota::reconcile(&transaction, self.node).await?,
                            );
                        }
                        review_changes.extend(
                            super::agent::review::command(&transaction, request, &output).await?,
                        );
                        if matches!(
                            request.command,
                            Command::SaveLocalModelSettings(_) | Command::RegisterLocalModel { .. }
                        ) && let Some(registry) = &self.local_models
                        {
                            let catalog = super::local_models::read(&transaction).await?;
                            model_configuration =
                                Some(registry.prepare(catalog.settings, catalog.definitions)?);
                        }
                        Ok((output, changes))
                    })(value)
                    .await
                }
                Err(error) => Err(error),
            };
            let (mut result, changes) = match outcome {
                Ok((output, changes)) => (Ok(output), changes),
                Err(error) => {
                    if conflict::is_conflict(&error) {
                        return Err(error);
                    }
                    review_changes.clear();
                    transaction
                        .execute_batch("ROLLBACK TO command")
                        .await
                        .map_err(storage_error)?;
                    (Err(error), Vec::new())
                }
            };
            if !approval {
                transaction
                    .execute_batch("RELEASE command")
                    .await
                    .map_err(storage_error)?;
            }
            let mut envelopes = record(&transaction, self.node, changes).await?;
            let receipt = async |result: &Response| -> Result<(), Fault> {
                if request.command.durable() {
                    transaction.execute(
                    "UPDATE requests SET status='completed',result=?3 WHERE caller=?1 AND id=?2",
                    params![&caller.0[..], request.id.to_string(), serde_json::to_string(result).map_err(storage_error)?],
                ).await.map_err(storage_error)?;
                }
                Ok(())
            };
            receipt(&result).await?;
            #[cfg(any(test, feature = "test-support"))]
            if approval && result.is_ok() {
                super::agent::approvals::fixture::wait(&self.approval_commit, false, &transaction)
                    .await;
            }
            if approval
                && let Ok(output) = &result
                && let Err(error) =
                    super::agent::approvals::claim(&transaction, &self.approvals, output).await
            {
                // The receipt is inside this savepoint too: no stale Approved result or
                // session rule may survive when expiry won before the commit claim.
                transaction
                    .execute_batch("ROLLBACK TO command")
                    .await
                    .map_err(storage_error)?;
                result = Err(error);
                envelopes.clear();
                review_changes.clear();
                model_configuration = None;
                receipt(&result).await?;
            }
            if approval {
                if let Ok(Output::Approval(approved)) = &result {
                    envelopes = record(
                        &transaction,
                        self.node,
                        vec![
                            super::agent::activity::changed(&transaction, approved.session).await?,
                        ],
                    )
                    .await?;
                }
                transaction
                    .execute_batch("RELEASE command")
                    .await
                    .map_err(storage_error)?;
            }
            #[cfg(any(test, feature = "test-support"))]
            if approval && result.is_ok() {
                super::agent::approvals::fixture::wait(&self.approval_commit, true, &transaction)
                    .await;
            }
            let publication = publication.lock().await;
            conflict::commit(transaction).await?;
            if let Some(prepared) = model_configuration {
                self.local_models
                    .as_ref()
                    .expect("local model owner was prepared")
                    .apply(prepared);
            }
            match &mut result {
                Ok(Output::Conversation(history)) => {
                    history.sequence = self.feeds.sequence(history.page.session);
                }
                Ok(Output::TurnHistory(history)) => {
                    history.sequence = self.feeds.sequence(history.run.session);
                }
                _ => {}
            }
            for event in envelopes {
                match &event.event {
                    Event::SessionChanged(session) if session.archived => {
                        self.model_clients.evict(session.id)
                    }
                    Event::SessionsRemoved(ids) => {
                        for id in ids {
                            self.model_clients.evict(*id);
                        }
                    }
                    Event::PluginChanged(package) => {
                        super::agent::invalidate_package(self, &package.name).await
                    }
                    Event::PluginRemoved { name } => {
                        super::agent::invalidate_package(self, name).await
                    }
                    _ => {}
                }
                publication.send(events, event);
            }
            if let Ok(output) = &result {
                for (session, change) in review_changes {
                    self.feeds.publish(self.node, session, change);
                }
                super::agent::publish_command(self, output).await;
            }
            Ok(result)
        })
    }

    pub(super) fn finish_external<'a>(
        &'a mut self,
        caller: NodeId,
        request: &'a Request,
        result: Response,
        events: &'a broadcast::Sender<EventEnvelope>,
    ) -> sailry_link::Pending<'a, Response> {
        Box::pin(async move {
            if matches!(
                request.command,
                Command::WriteFile { .. } | Command::EditFile { .. } | Command::EditNotebook { .. }
            ) {
                self.relocations
                    .lock()
                    .unwrap()
                    .remove(&(caller, request.id));
                return match super::outcomes::complete(self, caller, request.id, &result).await {
                    Ok(()) => result,
                    Err(error) => {
                        super::outcomes::failed(self.node, caller, request.id, &error);
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "operation result could not be recorded; inspect the target before retrying",
                        ))
                    }
                };
            }
            self.finish_external_with(caller, request, result, events, async |_, _| Ok(None))
                .await
        })
    }

    pub(in crate::store) async fn finish_external_with(
        &mut self,
        caller: NodeId,
        request: &Request,
        mut result: Response,
        events: &broadcast::Sender<EventEnvelope>,
        finish: impl AsyncFnOnce(&Connection, &mut Response) -> Result<Option<Event>, Fault>,
    ) -> Response {
        if !request.command.durable() {
            return result;
        }
        self.end_removal(caller, request);
        self.relocations
            .lock()
            .unwrap()
            .remove(&(caller, request.id));
        let mut background = Vec::new();
        let mut deliveries = Vec::new();
        let publication = self.publication.clone();
        let recorded = (async || {
            let transaction = self.connection.unchecked_transaction().await.map_err(storage_error)?;
            let event = finish(&transaction, &mut result).await?;
            let event = match event {
                Some(event) => Some(event),
                None => {
                    super::worktrees::finish(
                        &transaction,
                        &mut result,
                        matches!(request.command, Command::RegisterWorktree { .. }),
                    )
                    .await?
                }
            };
            let event = match event {
                Some(event) => Some(event),
                None => {
                    super::terminals::finish(&transaction, &request.command, &mut result).await?
                }
            };
            let event = match event {
                Some(event) => Some(event),
                None => {
                    super::plugins::finish(
                        &transaction,
                        self.profile.as_deref(),
                        &request.command,
                        &mut result,
                    )
                    .await?
                }
            };
            let event = match event {
                Some(event) => Some(event),
                None => {
                    super::projects::finish(&transaction, &request.command, &mut result).await?
                }
            };
            super::attachments::finish(&transaction, caller, &request.command, &result).await?;
            if let Some(task) =
                super::agent::background::finish_request(&transaction, request, &result).await?
            {
                if let Some(turn) =
                    super::agent::background::schedule(&transaction, self.node, task.session)
                        .await?
                {
                    deliveries.push(turn);
                }
                background.push(task);
            }
            let mut changes: Vec<_> = event.into_iter().collect();
            if let Some(Event::PluginChanged(summary)) = changes.first() {
                let info = super::plugins::get(&transaction, &summary.name)
                    .await?
                    .ok_or_else(|| storage_error("updated plugin is missing"))?;
                changes.extend(super::plugins::options::migrate(&transaction, &info).await?);
            }
            if self.profile.is_some()
                && changes
                    .iter()
                    .any(|event| matches!(event, Event::PluginChanged(_)))
                && super::knowledge::semantic::startup(
                    &transaction,
                    &sailry_link::CancellationToken::new(),
                )
                .await?
            {
                changes.push(Event::KnowledgeCollectionsChanged);
            }
            let envelopes = record(&transaction, self.node, changes).await?;
            let changed = transaction.execute(
                "UPDATE requests SET status='completed',result=?3 WHERE caller=?1 AND id=?2 AND status='admitted'",
                params![&caller.0[..], request.id.to_string(), serde_json::to_string(&result).map_err(storage_error)?],
            ).await.map_err(storage_error)?;
            if changed != 1 {
                return Err(storage_error("durable admission missing"));
            }
            let delivered = { let mut selected = Vec::new(); for value in deliveries
                .iter() { selected.push((async |turn: &QueuedTurn| {
                    Ok((
                        super::agent::runs::get(&transaction, turn.id).await?,
                        super::agent::background::list(&transaction, turn.session).await?,
                    ))
                })(value).await?); } Ok::<_,Fault>(selected) }?;
            let publication = publication.lock().await;
            crate::store::database::conflict::commit(transaction).await?;
            Ok::<_, Fault>((envelopes, delivered, publication))
        })().await;
        match recorded {
            Err(cause) => {
                super::outcomes::failed(self.node, caller, request.id, &cause);
                let mut error = Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "operation result could not be recorded; inspect the target before retrying",
                );
                if matches!(request.command, Command::RemoveProject { .. }) {
                    error.details = Some(FaultDetails::ProjectRemoval {
                        tasks_active: false,
                        terminals_may_be_closed: true,
                    });
                }
                Err(error)
            }
            Ok((envelopes, delivered, publication)) => {
                super::agent::background::publish(self, background).await;
                for (run, tasks) in delivered {
                    super::agent::publish_run(self, run).await;
                    super::agent::background::publish(self, tasks).await;
                }
                for event in envelopes {
                    match &event.event {
                        Event::PluginChanged(package) => {
                            super::agent::invalidate_package(self, &package.name).await
                        }
                        Event::PluginRemoved { name } => {
                            super::agent::invalidate_package(self, name).await
                        }
                        _ => {}
                    }
                    publication.send(events, event);
                }
                result
            }
        }
    }
}

async fn admit_in(
    transaction: &Connection,
    caller: NodeId,
    request: &Request,
) -> Result<Option<Response>, Fault> {
    let encoded = zeroize::Zeroizing::new(encode(request)?);
    if encoded.len() > MAX_FRAME_BYTES {
        return Err(Fault::invalid("request exceeds the frame limit"));
    }
    if !request.command.durable() {
        return Ok(None);
    }
    let body = if matches!(
        request.command,
        Command::PutCredential { .. }
            | Command::SaveSsh { .. }
            | Command::SaveDatabase { .. }
            | Command::CompleteProviderLogin { .. }
            | Command::SavePluginSettings { .. }
            | Command::StartConnectorLogin { .. }
            | Command::UpdateConnection { .. }
            | Command::SubmitConnectorInput { .. }
            | Command::SavePluginMcp { .. }
            | Command::InstallMcp { .. }
            | Command::SaveProvider { .. }
            | Command::ImportSession(_)
    ) {
        // Keep replay identity without making the admission ledger another secret store.
        format!("credential-v1:{}", blake3::hash(&encoded).to_hex()).into_bytes()
    } else if matches!(
        request.command,
        Command::WriteFile { .. }
            | Command::EditFile { .. }
            | Command::EditNotebook { .. }
            | Command::WriteArtifact(_)
    ) {
        // The ledger owns replay identity, not another copy of project content.
        format!("file-v1:{}", blake3::hash(&encoded).to_hex()).into_bytes()
    } else if matches!(
        request.command,
        Command::RunCommand { .. }
            | Command::RunSsh { .. }
            | Command::QueryDatabase { .. }
            | Command::UseExternalBrowser { .. }
            | Command::UseBrowser { .. }
            | Command::UseMedia { .. }
    ) {
        format!("command-v1:{}", blake3::hash(&encoded).to_hex()).into_bytes()
    } else {
        encoded.to_vec()
    };
    let previous: Option<(Vec<u8>, String, Option<String>)> = (match (async {
        let mut statement = transaction
            .prepare("SELECT body,status,result FROM requests WHERE caller=?1 AND id=?2")
            .await?;
        let row = statement
            .query_row(params![&caller.0[..], request.id.to_string()])
            .await?;
        let mapped: turso::Result<_> =
            (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
        mapped
    })
    .await
    {
        Ok(value) => Ok(Some(value)),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(error),
    })
    .map_err(storage_error)?;
    if let Some((previous, status, result)) = previous {
        let previous = (|| {
            if previous != body {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "request identifier was already used for different content",
                ));
            }
            if status == "completed" {
                return Ok(Some(super::outcomes::decode(
                    &result.ok_or_else(|| storage_error("missing result"))?,
                )?));
            }
            Ok(Some(Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "request outcome requires reconciliation; automatic replay is disabled",
            ))))
        })();
        return previous;
    }
    transaction
        .execute(
            "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
            params![&caller.0[..], request.id.to_string(), body],
        )
        .await
        .map_err(storage_error)?;
    Ok(None)
}

pub(super) async fn record(
    db: &Connection,
    node: NodeId,
    changes: Vec<Event>,
) -> Result<Vec<EventEnvelope>, Fault> {
    let mut envelopes = Vec::with_capacity(changes.len());
    for event in changes {
        let cursor = counters::next(db, counters::Namespace::Events).await?;
        db.execute(
            "INSERT INTO events(cursor,body) VALUES(?1,?2)",
            params![cursor as i64, encode(&event)?],
        )
        .await
        .map_err(storage_error)?;
        envelopes.push(EventEnvelope {
            node,
            cursor,
            event,
        });
    }
    Ok(envelopes)
}

pub(super) fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, Fault> {
    serde_json::to_vec(value).map_err(storage_error)
}
pub(super) fn storage_error(error: impl std::fmt::Display + 'static) -> Fault {
    let error = &error as &dyn std::any::Any;
    let native = error.downcast_ref::<turso::Error>().or_else(|| {
        match error.downcast_ref::<crate::Error>() {
            Some(crate::Error::Database(error)) => Some(error),
            _ => None,
        }
    });
    if native.is_some_and(conflict::native) {
        let mut fault = Fault::new(ErrorCode::Busy, "Node storage transaction conflicted");
        fault.details = Some(FaultDetails::StorageConflict);
        fault
    } else {
        Fault::new(ErrorCode::Internal, "Node storage operation failed")
    }
}

pub(super) async fn validate_backup(db: &Connection, node: NodeId) -> Result<(), Fault> {
    schema::validate_backup(db, node)
        .await
        .map_err(storage_error)?;
    validation::stored(db).await.map_err(storage_error)?;
    let integrity: String = db
        .prepare("PRAGMA integrity_check")
        .await
        .map_err(storage_error)?
        .query_row(())
        .await
        .map_err(storage_error)?
        .get(0)
        .map_err(storage_error)?;
    if integrity != "ok" {
        return Err(Fault::invalid("backup database integrity check failed"));
    }
    Ok(())
}

pub(super) async fn upgrade_backup(
    path: &Path,
    node: NodeId,
) -> Result<Option<std::path::PathBuf>, Fault> {
    let (handle, connection, backup) = startup::upgrade(path, node, None)
        .await
        .map_err(storage_error)?;
    driver::checkpoint(&connection)
        .await
        .map_err(storage_error)?;
    drop(connection);
    drop(handle);
    Ok(backup)
}
