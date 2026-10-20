use std::{path::Path, time::Duration};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sailry_link::{Admission, Response};
use sailry_protocol::*;
use tokio::sync::{broadcast, oneshot};

use super::{commands, external::Work, mutations};
use crate::Error;

mod relocation;
mod removal;
pub(super) mod schema;
pub(super) mod validation;

const APPLICATION_ID: i64 = 0x5341_494c;

pub(super) struct Database {
    pub(super) connection: Connection,
    pub(super) node: NodeId,
    pub(super) feeds: super::agent::Feeds,
    pub(super) approvals: super::agent::approvals::Waiting,
    pub(super) questions: super::agent::questions::Waiting,
    pub(super) profile: Option<std::path::PathBuf>,
    pub(super) network_link: Option<sailry_link::LinkHandle>,
    pub(super) network_request: Option<RequestId>,
    removals: std::collections::BTreeMap<WorktreeId, removal::Pending>,
    relocations: std::collections::BTreeMap<(NodeId, RequestId), Vec<std::path::PathBuf>>,
}

impl Database {
    pub(super) fn worktree_root(&self, id: WorktreeId) -> Result<std::path::PathBuf, Fault> {
        self.check_removing(id)?;
        let path: Option<String> = self
            .connection
            .query_row(
                "SELECT path FROM worktrees WHERE id=?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let root = path.map(std::path::PathBuf::from).ok_or_else(|| {
            Fault::new(ErrorCode::NotFound, "worktree does not exist on this Node")
        })?;
        super::worktrees::present(&self.connection, id)?;
        super::worktrees::materialize_session(
            &self.connection,
            self.profile.as_deref(),
            id,
            &root,
        )?;
        Ok(root)
    }

    pub(super) fn resolve_credential(
        &self,
        reference: &CredentialRef,
        provider: ProviderId,
        authentication: Authentication,
    ) -> Result<Secret, Fault> {
        self.credential(reference, provider, authentication)
            .map(|(_, secret)| secret)
    }

    pub(super) fn credential(
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
    }

    pub(super) fn addresses(&self) -> Result<Vec<sailry_link::EndpointAddr>, Fault> {
        let mut query = self
            .connection
            .prepare("SELECT address FROM link_addresses ORDER BY identity")
            .map_err(storage_error)?;
        query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?
            .map(|row| {
                serde_json::from_str(&row.map_err(storage_error)?)
                    .map_err(|_| storage_error("invalid peer address"))
            })
            .collect()
    }

    pub(super) fn remember(&self, address: sailry_link::EndpointAddr) -> Result<(), Fault> {
        let body = serde_json::to_string(&address).map_err(storage_error)?;
        if body.len() > 8192 || address.addrs.len() > 16 {
            return Err(Fault::invalid("peer address exceeds limits"));
        }
        self.connection.execute("INSERT INTO link_addresses(identity,address) VALUES(?1,?2) ON CONFLICT(identity) DO UPDATE SET address=excluded.address", params![&address.id.as_bytes()[..], body]).map_err(storage_error)?;
        Ok(())
    }
    pub(super) fn peers(&self) -> Result<Vec<NodeId>, Fault> {
        let mut query = self
            .connection
            .prepare("SELECT identity FROM link_peers ORDER BY identity")
            .map_err(storage_error)?;
        query
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(storage_error)?
            .map(|row| {
                let bytes = row.map_err(storage_error)?;
                Ok(NodeId(
                    bytes
                        .try_into()
                        .map_err(|_| storage_error("invalid peer identity"))?,
                ))
            })
            .collect()
    }

    pub(super) fn set_trust(&self, peer: NodeId, trusted: bool) -> Result<(), Fault> {
        if trusted {
            let count: i64 = self
                .connection
                .query_row("SELECT count(*) FROM link_peers", [], |row| row.get(0))
                .map_err(storage_error)?;
            if count >= 128 && !self.peers()?.contains(&peer) {
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
                .map_err(storage_error)?;
        } else {
            self.connection
                .execute("DELETE FROM link_peers WHERE identity=?1", [&peer.0[..]])
                .map_err(storage_error)?;
        }
        Ok(())
    }

    pub(super) fn open(
        path: &Path,
        node: NodeId,
        profile: Option<std::path::PathBuf>,
    ) -> Result<Self, Error> {
        #[cfg(windows)]
        crate::profile::windows::prepare_database(path)?;
        if let Ok(metadata) = path.symlink_metadata() {
            if !metadata.is_file() || metadata.is_symlink() {
                return Err(Error::InvalidProfile {
                    path: path.to_owned(),
                    reason: "database must be a regular file",
                });
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() != 1 {
                    return Err(Error::InvalidProfile {
                        path: path.to_owned(),
                        reason: "database must not be hard-linked",
                    });
                }
            }
        }
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        connection.busy_timeout(Duration::from_secs(1))?;
        let schema = schema::prepare(&connection, path, node)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let transaction = connection.transaction()?;
        schema.initialize(&transaction)?;
        transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
        transaction.pragma_update(None, "user_version", 1)?;
        let stored: Option<Vec<u8>> = transaction
            .query_row("SELECT identity FROM node WHERE singleton=1", [], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(stored) = stored {
            if stored != node.0 {
                return Err(Error::Worker(
                    "Node identity does not match its database".into(),
                ));
            }
        } else {
            transaction.execute(
                "INSERT INTO node(singleton,identity) VALUES(1,?1)",
                [&node.0[..]],
            )?;
        }
        // An admission with no committed result is uncertain. Never replay it automatically.
        transaction.execute(
            "UPDATE requests SET status='unknown' WHERE status='admitted'",
            [],
        )?;
        super::agent::recover(&transaction).map_err(|error| Error::Worker(error.to_string()))?;
        super::dispatch::jobs::recover(&transaction, node)
            .map_err(|error| Error::Worker(error.to_string()))?;
        for mut info in super::terminals::list(&transaction)
            .map_err(|error| Error::Worker(error.to_string()))?
        {
            if info.status == terminal::Status::Running {
                info.status = terminal::Status::Stopped;
                info.owner = None;
                info.revision += 1;
                if let Some(event) = super::terminals::record(&transaction, &info)
                    .map_err(|error| Error::Worker(error.to_string()))?
                {
                    transaction.execute(
                        "INSERT INTO events(body) VALUES(?1)",
                        [encode(&event).map_err(|error| Error::Worker(error.to_string()))?],
                    )?;
                }
            }
        }
        transaction.commit()?;
        #[cfg(windows)]
        crate::profile::windows::validate_database(path)?;
        Ok(Self {
            connection,
            node,
            removals: Default::default(),
            profile,
            relocations: Default::default(),
            network_link: None,
            network_request: None,
            feeds: Default::default(),
            approvals: Default::default(),
            questions: Default::default(),
        })
    }

    pub(super) fn close(self) -> Result<(), Error> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        self.connection
            .close()
            .map_err(|(_, error)| Error::Database(error))
    }

    #[cfg(test)]
    pub(super) fn dispatch(
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
    }

    pub(super) fn dispatch_with(
        &mut self,
        caller: NodeId,
        request: Request,
        reply: oneshot::Sender<Result<Admission, Fault>>,
        events: &broadcast::Sender<EventEnvelope>,
        access: crate::files::Access,
    ) -> Option<Work> {
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
        let previous = match self.admit(caller, &request) {
            Ok(previous) => previous,
            Err(error) => {
                let _ = reply.send(Err(error));
                return None;
            }
        };
        let (completion, result) = oneshot::channel();
        let plugin_command = super::plugins::command::resolve(self, &request);
        // admit() has committed before this receipt can reach either transport.
        let _ = reply.send(Ok(Admission {
            receipt,
            completion: result,
        }));
        let output = if let Some(previous) = previous {
            previous
        } else if request.command.durable()
            && let Err(error) = self.check_plugin(caller, &request)
        {
            self.finish_external(caller, &request, Err(error), events)
        } else if let Err(error) = self.check_removal_scope(&request.command) {
            self.finish_external(caller, &request, Err(error), events)
        } else if let Err(error) = plugin_command {
            self.finish_external(caller, &request, Err(error), events)
        } else if let Ok(Some(child)) = plugin_command {
            return Some(Work::PluginExecution(
                super::plugins::execution::Execution {
                    caller,
                    request,
                    input: super::plugins::execution::Input::Command(child),
                    reply: completion,
                },
            ));
        } else if matches!(
            request.command,
            Command::ReadNetworkSettings | Command::SaveNetworkSettings { .. }
        ) {
            match super::network::prepare(self, caller, request, completion) {
                Ok(execution) => return Some(Work::Network(execution)),
                Err(failure) => {
                    let (request, completion, error) = *failure;
                    let result = self.finish_external(caller, &request, Err(error), events);
                    let _ = completion.send(result);
                    return None;
                }
            }
        } else if matches!(
            request.command,
            Command::BeginProviderLogin { .. }
                | Command::CompleteProviderLogin { .. }
                | Command::CancelProviderLogin { .. }
                | Command::BeginMcpLogin { .. }
                | Command::CompleteMcpLogin { .. }
                | Command::CancelMcpLogin { .. }
                | Command::BeginPluginLogin { .. }
                | Command::BeginPluginRegistration { .. }
                | Command::CompletePluginLogin { .. }
                | Command::CancelPluginLogin { .. }
                | Command::ImportGithubLogin { .. }
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
                super::ssh::prepare(self, &request.command).map(Some)
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
                Err(error) => self.finish_external(caller, &request, Err(error), events),
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
                super::databases::prepare(self, &request.command).map(Some)
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
                Err(error) => self.finish_external(caller, &request, Err(error), events),
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
            match super::plugins::execution::prepare(self, caller, &request) {
                Ok(input) => {
                    return Some(Work::PluginExecution(
                        super::plugins::execution::Execution {
                            caller,
                            request,
                            input,
                            reply: completion,
                        },
                    ));
                }
                Err(error) => self.finish_external(caller, &request, Err(error), events),
            }
        } else if matches!(
            request.command,
            Command::CloseTerminal { .. }
                | Command::RestoreBackupFile { .. }
                | Command::ExportBackup { .. }
                | Command::RestoreBackup { .. }
                | Command::StartCodeServer { .. }
                | Command::StopCodeServer { .. }
                | Command::InstallKnowledgeModel { .. }
                | Command::InstallOcrModel { .. }
                | Command::DeleteOcrModel { .. }
                | Command::DeleteKnowledgeModel { .. }
                | Command::InstallCodeServer { .. }
                | Command::UninstallCodeServer { .. }
                | Command::UpdateCodeRules
                | Command::UseExternalBrowser { .. }
                | Command::UseBrowser { .. }
                | Command::UseComputer { .. }
                | Command::UseMedia { .. }
        ) {
            match super::resources::prepare(self, &request) {
                Ok(()) => {
                    return Some(Work::Resources(super::resources::Execution {
                        caller,
                        request,
                        reply: completion,
                    }));
                }
                Err(error) => self.finish_external(caller, &request, Err(error), events),
            }
        } else if matches!(request.command, Command::RunCommand { .. }) {
            match super::processes::prepare(self, &request).and_then(|launch| {
                if let Some(task) =
                    super::agent::background::command_started(&self.connection, &request)?
                {
                    super::agent::background::publish(self, [task]);
                }
                Ok(launch)
            }) {
                Ok(launch) => {
                    return Some(Work::Process(super::processes::Execution {
                        caller,
                        request,
                        launch,
                        reply: completion,
                    }));
                }
                Err(error) => self.finish_external(caller, &request, Err(error), events),
            }
        } else if matches!(request.command, Command::CreateSessionAt { .. }) {
            match super::forwarding::prepare(self, caller, &request) {
                Ok(forwarded) => {
                    return Some(Work::Session(super::forwarding::Transfer {
                        caller,
                        request,
                        forwarded: Box::new(forwarded),
                        reply: completion,
                    }));
                }
                Err(error) => self.finish_external(caller, &request, Err(error), events),
            }
        } else if let Some(target) = mutations::target(&request.command) {
            let roots = match target {
                mutations::Target::NewProject => {
                    let Command::CreateProject(draft) = &request.command else {
                        unreachable!()
                    };
                    super::projects::prepare(draft).and_then(|path| {
                        if matches!(
                            draft.source,
                            sailry_protocol::projects::Source::Clone { .. }
                        ) {
                            let parent = path.parent().unwrap().to_owned();
                            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                            self.begin_relocation(caller, request.id, &[(parent, name)], access)?;
                        }
                        Ok((path.clone(), path))
                    })
                }
                mutations::Target::ManagedWorktree { project, source } => {
                    super::worktrees::select(&self.connection, project, Some(source))
                        .and_then(|source| self.worktree_root(source))
                        .and_then(|source| {
                            super::worktrees::project_root(&self.connection, project)
                                .map(|root| (source, crate::workspaces::worktrees(&root)))
                        })
                }
                mutations::Target::Host => {
                    let validation = if let Command::ManageFiles(action) = &request.command {
                        crate::files::browser::targets(action).and_then(|paths| {
                            self.begin_relocation(caller, request.id, &paths, access)
                        })
                    } else if let Command::RemoveProject { expected } = &request.command {
                        self.begin_project_removal(caller, &request, expected)
                    } else {
                        Ok(())
                    };
                    validation.map(|()| (std::path::PathBuf::new(), std::path::PathBuf::new()))
                }
                mutations::Target::Plugin => {
                    Ok((std::path::PathBuf::new(), std::path::PathBuf::new()))
                }
                mutations::Target::Worktree(id) => {
                    self.worktree_root(id).map(|root| (root.clone(), root))
                }
                mutations::Target::Project(id) => {
                    super::worktrees::project_root(&self.connection, id)
                        .map(|root| (root.clone(), root))
                }
                mutations::Target::Worktrees { source, target } => self
                    .worktree_root(source)
                    .and_then(|source| self.worktree_root(target).map(|target| (source, target))),
            };
            let roots = roots.and_then(|(source, target)| {
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
                    super::plugins::check_install(&self.connection, name, *expected_revision)?;
                }
                if let Command::SetPluginEnabled {
                    name,
                    expected_revision,
                    enabled: true,
                } = &request.command
                {
                    super::plugins::prepare_enable(&self.connection, name, *expected_revision)?;
                }
                if let Command::RemoveWorktree { worktree, .. } = request.command {
                    self.begin_removal(caller, &request, worktree, &roots.target)?;
                }
                let mut paths = roots.entries(&request.command);
                if let Command::ApplyTurnHunk(selection) = &request.command {
                    super::agent::checkpoints::hunks::prepare(self, selection, &roots.target)?;
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
                    )?;
                    paths.push((roots.target.clone(), content.file.path));
                }
                if !paths.is_empty() {
                    self.begin_relocation(caller, request.id, &paths, access)?;
                }
                Ok(roots)
            });
            match roots {
                Ok(roots) => {
                    return Some(Work::Mutation(mutations::Mutation {
                        caller,
                        request,
                        roots,
                        reply: completion,
                    }));
                }
                Err(error) => self.finish_external(caller, &request, Err(error), events),
            }
        } else {
            self.execute(caller, &request, events)
        };
        let _ = completion.send(output);
        None
    }

    fn admit(&mut self, caller: NodeId, request: &Request) -> Result<Option<Response>, Fault> {
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
                | Command::CompleteMcpLogin { .. }
                | Command::CompleteProviderLogin { .. }
                | Command::BeginPluginLogin { .. }
                | Command::CompletePluginLogin { .. }
                | Command::SavePluginSettings { .. }
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
                | Command::UseComputer { .. }
                | Command::UseMedia { .. }
        ) {
            format!("command-v1:{}", blake3::hash(&encoded).to_hex()).into_bytes()
        } else {
            encoded.to_vec()
        };
        let transaction = self.connection.transaction().map_err(storage_error)?;
        let previous: Option<(Vec<u8>, String, Option<String>)> = transaction
            .query_row(
                "SELECT body,status,result FROM requests WHERE caller=?1 AND id=?2",
                params![&caller.0[..], request.id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((previous, status, result)) = previous {
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
            return Ok(Some(Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "request outcome requires reconciliation; automatic replay is disabled",
            ))));
        }
        transaction
            .execute(
                "INSERT INTO requests(caller,id,body,status) VALUES(?1,?2,?3,'admitted')",
                params![&caller.0[..], request.id.to_string(), body],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(None)
    }

    fn execute(
        &mut self,
        caller: NodeId,
        request: &Request,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        let transaction = self.connection.transaction().map_err(storage_error)?;
        // Commands validate before mutation. A savepoint also rolls back partial business errors.
        transaction
            .execute_batch("SAVEPOINT command")
            .map_err(storage_error)?;
        let mut review_changes = super::agent::review::Changes::new();
        let outcome = if matches!(request.command, Command::PluginTransaction { .. }) {
            super::plugins::transaction::execute(
                &transaction,
                self.node,
                caller,
                self.profile.as_deref(),
                request,
            )
        } else {
            commands::execute(
                &transaction,
                self.node,
                caller,
                self.profile.as_deref(),
                request,
            )
            .map(|(output, event)| (output, event.into_iter().collect()))
        };
        let outcome = outcome.and_then(|(output, mut changes)| {
            if self.profile.is_some()
                && changes
                    .iter()
                    .any(|event| matches!(event, Event::PluginChanged(_)))
                && super::knowledge::semantic::startup(
                    &transaction,
                    &sailry_link::CancellationToken::new(),
                )?
            {
                changes.push(Event::KnowledgeCollectionsChanged);
            }
            if self.profile.is_some()
                && super::knowledge::runtime::storage::changes(&request.command)
                && super::knowledge::semantic::changed(
                    &transaction,
                    request,
                    &sailry_link::CancellationToken::new(),
                )?
            {
                changes.push(Event::KnowledgeCollectionsChanged);
            }
            if super::providers::quota::affected(&request.command) {
                changes.extend(super::providers::quota::reconcile(&transaction, self.node)?);
            }
            review_changes.extend(super::agent::review::command(
                &transaction,
                request,
                &output,
            )?);
            Ok((output, changes))
        });
        let (mut result, changes) = match outcome {
            Ok((output, changes)) => (Ok(output), changes),
            Err(error) => {
                review_changes.clear();
                transaction
                    .execute_batch("ROLLBACK TO command")
                    .map_err(storage_error)?;
                (Err(error), Vec::new())
            }
        };
        transaction
            .execute_batch("RELEASE command")
            .map_err(storage_error)?;
        let envelopes = record(&transaction, self.node, changes)?;
        if request.command.durable() {
            transaction
                .execute(
                    "UPDATE requests SET status='completed',result=?3 WHERE caller=?1 AND id=?2",
                    params![
                        &caller.0[..],
                        request.id.to_string(),
                        serde_json::to_string(&result).map_err(storage_error)?
                    ],
                )
                .map_err(storage_error)?;
        }
        transaction.commit().map_err(storage_error)?;
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
                Event::PluginChanged(package) => {
                    super::agent::invalidate_package(self, &package.name)
                }
                Event::PluginRemoved { name } => super::agent::invalidate_package(self, name),
                _ => {}
            }
            let _ = events.send(event);
        }
        if let Ok(output) = &result {
            for (session, change) in review_changes {
                self.feeds.publish(self.node, session, change);
            }
            super::agent::publish_command(self, output);
        }
        result
    }

    pub(super) fn finish_external(
        &mut self,
        caller: NodeId,
        request: &Request,
        result: Response,
        events: &broadcast::Sender<EventEnvelope>,
    ) -> Response {
        self.finish_external_with(caller, request, result, events, |_, _| Ok(None))
    }

    pub(in crate::store) fn finish_external_with(
        &mut self,
        caller: NodeId,
        request: &Request,
        mut result: Response,
        events: &broadcast::Sender<EventEnvelope>,
        finish: impl FnOnce(&Connection, &mut Response) -> Result<Option<Event>, Fault>,
    ) -> Response {
        if !request.command.durable() {
            return result;
        }
        self.end_removal(caller, request);
        self.relocations.remove(&(caller, request.id));
        let mut background = Vec::new();
        let mut deliveries = Vec::new();
        let recorded = (|| {
            let transaction = self.connection.transaction().map_err(storage_error)?;
            let event = finish(&transaction, &mut result)?;
            let event = match event {
                Some(event) => Some(event),
                None => super::worktrees::finish(
                    &transaction,
                    &mut result,
                    matches!(request.command, Command::RegisterWorktree { .. }),
                )?,
            };
            let event = match event {
                Some(event) => Some(event),
                None => super::terminals::finish(&transaction, &request.command, &mut result)?,
            };
            let event = match event {
                Some(event) => Some(event),
                None => super::plugins::finish(&transaction, &request.command, &mut result)?,
            };
            let event = match event {
                Some(event) => Some(event),
                None => super::projects::finish(&transaction, &request.command, &mut result)?,
            };
            super::attachments::finish(&transaction, caller, &request.command, &result)?;
            if let Some(task) =
                super::agent::background::finish_request(&transaction, request, &result)?
            {
                if let Some(turn) =
                    super::agent::background::schedule(&transaction, self.node, task.session)?
                {
                    deliveries.push(turn);
                }
                background.push(task);
            }
            let mut changes: Vec<_> = event.into_iter().collect();
            if let Some(Event::PluginChanged(summary)) = changes.first() {
                let info = super::plugins::get(&transaction, &summary.name)?
                    .ok_or_else(|| storage_error("updated plugin is missing"))?;
                changes.extend(super::plugins::options::migrate(&transaction, &info)?);
            }
            if self.profile.is_some()
                && changes
                    .iter()
                    .any(|event| matches!(event, Event::PluginChanged(_)))
                && super::knowledge::semantic::startup(
                    &transaction,
                    &sailry_link::CancellationToken::new(),
                )?
            {
                changes.push(Event::KnowledgeCollectionsChanged);
            }
            let envelopes = record(&transaction, self.node, changes)?;
            let changed = transaction.execute(
                "UPDATE requests SET status='completed',result=?3 WHERE caller=?1 AND id=?2 AND status='admitted'",
                params![&caller.0[..], request.id.to_string(), serde_json::to_string(&result).map_err(storage_error)?],
            ).map_err(storage_error)?;
            if changed != 1 {
                return Err(storage_error("durable admission missing"));
            }
            let delivered = deliveries
                .iter()
                .map(|turn| {
                    Ok((
                        super::agent::runs::get(&transaction, turn.id)?,
                        super::agent::background::list(&transaction, turn.session)?,
                    ))
                })
                .collect::<Result<Vec<_>, Fault>>()?;
            transaction.commit().map_err(storage_error)?;
            Ok::<_, Fault>((envelopes, delivered))
        })();
        match recorded {
            Err(_) => {
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
            Ok((envelopes, delivered)) => {
                super::agent::background::publish(self, background);
                for (run, tasks) in delivered {
                    super::agent::publish_run(self, run);
                    super::agent::background::publish(self, tasks);
                }
                for event in envelopes {
                    match &event.event {
                        Event::PluginChanged(package) => {
                            super::agent::invalidate_package(self, &package.name)
                        }
                        Event::PluginRemoved { name } => {
                            super::agent::invalidate_package(self, name)
                        }
                        _ => {}
                    }
                    let _ = events.send(event);
                }
                result
            }
        }
    }
}

pub(super) fn record(
    db: &Connection,
    node: NodeId,
    changes: Vec<Event>,
) -> Result<Vec<EventEnvelope>, Fault> {
    let mut envelopes = Vec::with_capacity(changes.len());
    for event in changes {
        db.execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
            .map_err(storage_error)?;
        envelopes.push(EventEnvelope {
            node,
            cursor: db.last_insert_rowid() as u64,
            event,
        });
    }
    Ok(envelopes)
}

pub(super) fn encode(value: &impl serde::Serialize) -> Result<Vec<u8>, Fault> {
    serde_json::to_vec(value).map_err(storage_error)
}
pub(super) fn storage_error(_: impl std::fmt::Display) -> Fault {
    Fault::new(ErrorCode::Internal, "Node storage operation failed")
}

pub(super) fn validate_backup(db: &Connection, node: NodeId) -> Result<(), Fault> {
    schema::validate_backup(db, node).map_err(storage_error)?;
    validation::stored(db).map_err(storage_error)?;
    let integrity: String = db
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(storage_error)?;
    if integrity != "ok" {
        return Err(Fault::invalid("backup database integrity check failed"));
    }
    let mut foreign = db
        .prepare("PRAGMA foreign_key_check")
        .map_err(storage_error)?;
    if foreign
        .query([])
        .map_err(storage_error)?
        .next()
        .map_err(storage_error)?
        .is_some()
    {
        return Err(Fault::invalid("backup database has invalid references"));
    }
    Ok(())
}
