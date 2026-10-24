//! Startup grants bind one exact server descriptor and reuse durable Node admission.
use super::{
    Ingress,
    database::{Database, encode, storage_error},
};
use sailry_link::{Admission, CancellationToken};
use sailry_protocol::{Command, ErrorCode, Fault, Output, Request, WorktreeId, code::Startup};
use turso::{Connection, params};

pub(super) async fn approved(db: &Connection, startup: &Startup) -> Result<bool, Fault> {
    let body = encode(startup)?;
    db.prepare("SELECT EXISTS(SELECT 1 FROM code_startups WHERE worktree=?1 AND root=?2 AND server=?3 AND body=?4)")
        .await.map_err(storage_error)?
        .query_row(params![startup.worktree.to_string(),startup.root.as_str(),startup.server.as_str(),body])
        .await.map_err(storage_error)?.get(0).map_err(storage_error)
}

pub(super) fn configures(command: &Command) -> bool {
    matches!(
        command,
        Command::SaveCodeSettings { .. }
            | Command::SetPluginEnabled { .. }
            | Command::RemovePlugin { .. }
            | Command::InstallPlugin { .. }
            | Command::InstallPluginUpload { .. }
            | Command::InstallPluginSource { .. }
            | Command::InstallBundledPlugin { .. }
            | Command::InstallSkill { .. }
            | Command::InstallRegistrySkill { .. }
            | Command::InstallSkillUpload { .. }
            | Command::InstallMcp { .. }
    )
}

pub(super) async fn grant(db: &Connection, startup: &Startup) -> Result<(), Fault> {
    db.execute("INSERT INTO code_startups(worktree,root,server,body) VALUES(?1,?2,?3,?4) ON CONFLICT(worktree,root,server) DO UPDATE SET body=excluded.body",params![startup.worktree.to_string(),startup.root.as_str(),startup.server.as_str(),encode(startup)?]).await.map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn startup_action(
    database: &Database,
    request: &Request,
) -> Result<sailry_protocol::plugin::Action, Fault> {
    let Some(context) = &request.plugin else {
        return Ok(sailry_protocol::plugin::Action::ManageCode);
    };
    let Some(turn) = context.turn else {
        return Ok(sailry_protocol::plugin::Action::ManageCode);
    };
    let binding: Option<Vec<u8>> = {
        let mut statement = database
            .connection
            .prepare(
                "SELECT binding FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1",
            )
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![turn.to_string(), request.id.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    let binding: Option<super::agent::ApprovalBinding> = binding
        .map(|binding| serde_json::from_slice(&binding).map_err(storage_error))
        .transpose()?
        .flatten();
    Ok(
        if binding.is_some_and(|binding| {
            binding.owner == context.package && binding.operation.is_some_and(crate::code::handles)
        }) {
            sailry_protocol::plugin::Action::ReadCode
        } else {
            sailry_protocol::plugin::Action::ManageCode
        },
    )
}

pub(super) async fn prepare(database: &Database, request: &Request) -> Result<(), Fault> {
    if !matches!(request.command, Command::StartCodeServer { .. }) {
        if matches!(
            request.command,
            Command::InstallCodeServer { .. }
                | Command::UninstallCodeServer { .. }
                | Command::StopCodeServer { .. }
        ) {
            management::authorize(database, request).await?;
        } else {
            controller(request)?;
        }
        return match &request.command {
            Command::InstallCodeServer { install } => {
                if install.package.is_empty()
                    || install.version.is_empty()
                    || install.revision.len() != 64
                {
                    return Err(Fault::invalid("invalid language install descriptor"));
                }
                Ok(())
            }
            Command::UninstallCodeServer { package, version } => {
                crate::code::validate_package(package, version)
            }
            Command::StopCodeServer { startup } => descriptor(database, startup).await,
            Command::UpdateCodeRules => Ok(()),
            _ => unreachable!("language lifecycle expected"),
        };
    }
    let Command::StartCodeServer { startup } = &request.command else {
        unreachable!("language startup expected")
    };
    if !settings(&database.connection).await?.enabled {
        return Err(crate::code::disabled());
    }
    descriptor(database, startup).await?;
    if let Some(context) = &request.plugin {
        let turn = context.turn.ok_or_else(|| {
            Fault::new(
                ErrorCode::PermissionDenied,
                "callbacks cannot start language servers",
            )
        })?;
        if context.worktree != Some(startup.worktree) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "language startup is outside the captured worktree",
            ));
        }
        let record: Option<(Vec<u8>, Vec<u8>)> = {
            let mut statement = database.connection.prepare("SELECT body,binding FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1").await.map_err(storage_error)?;
            match statement
                .query_row(params![turn.to_string(), request.id.to_string()])
                .await
            {
                Ok(row) => {
                    let mapped: turso::Result<_> =
                        (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error).into()),
            }
        };
        let (body, binding) = record.ok_or_else(|| {
            Fault::new(
                ErrorCode::PermissionDenied,
                "language startup has no claimed approval",
            )
        })?;
        let approval: sailry_protocol::conversation::Approval =
            serde_json::from_slice(&body).map_err(storage_error)?;
        let binding: Option<super::agent::ApprovalBinding> =
            serde_json::from_slice(&binding).map_err(storage_error)?;
        let manual = binding.as_ref().is_some_and(|binding| {
            binding.owner == context.package
                && binding.operation == Some(sailry_protocol::tool::Operation::StartCodeServer)
        });
        if context.session != Some(approval.session)
            || context.invocation.is_some()
            || !binding.is_some_and(|binding| {
                binding.owner == context.package
                    && (manual || binding.operation.is_some_and(crate::code::handles))
            })
        {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "language startup belongs to another captured call or package",
            ));
        }
        if manual {
            management::authorize(database, request).await?;
        } else if approval.state != sailry_protocol::conversation::ApprovalState::Approved
            || approval.startup.as_ref() != Some(startup)
        {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "language startup differs from the approved descriptor",
            ));
        }
    }
    // A direct authenticated controller sends this explicit consent command.
    // Agents and callbacks retain a plugin context and must claim their original approval.
    grant(&database.connection, startup).await
}

async fn descriptor(database: &Database, startup: &Startup) -> Result<(), Fault> {
    database.worktree_root(startup.worktree).await?;
    crate::files::path::components(&startup.root, true)?;
    if startup.server.is_empty()
        || startup.server.len() > 256
        || startup.executable.is_empty()
        || startup.executable.len() > 4096
        || startup.revision.len() != 64
        || !startup
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Fault::invalid("invalid language startup descriptor"));
    }
    Ok(())
}

pub(super) fn controller(request: &Request) -> Result<(), Fault> {
    if request
        .plugin
        .as_ref()
        .is_some_and(|context| !context.controller_settings())
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "language lifecycle changes require explicit controller consent",
        ));
    }
    Ok(())
}

impl Ingress {
    pub(crate) async fn code_approved(&self, startup: Startup) -> Result<bool, Fault> {
        let (reply, response) = tokio::sync::oneshot::channel();
        self.send_internal(super::Job::CodeApproved { startup, reply })
            .await?;
        response.await.map_err(|_| super::unavailable())?
    }

    pub(crate) async fn prepare_code(
        &self,
        worktree: WorktreeId,
        query: &sailry_protocol::code::Query,
    ) -> Result<Option<Startup>, Fault> {
        let root = self.worktree_root(worktree).await?;
        self.code.prepare(worktree, root, query).await
    }

    pub(super) async fn read_code(
        &self,
        caller: sailry_protocol::NodeId,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let output = match &request.command {
            Command::ReadCode {
                worktree,
                query,
                overlay,
            } => {
                let active = self.code_queries.begin(caller, request.id, &self.closed)?;
                Output::Code(
                    active
                        .run(async {
                            let root = self.worktree_root(*worktree).await?;
                            self.code.prepare(*worktree, root.clone(), query).await?;
                            let mut result = self
                                .code
                                .query(*worktree, root, query, overlay.as_ref(), &active.stop)
                                .await?;
                            if let Some(startup) = &result.startup {
                                result.startup_approved =
                                    self.code_approved(startup.clone()).await?;
                            }
                            if let Some(name) = &result.package {
                                result.package_info = self.code.package(name)?;
                            }
                            crate::code::limit(&mut result)?;
                            Ok(result)
                        })
                        .await?,
                )
            }
            Command::ListCodeServers { worktree } => {
                self.worktree_root(*worktree).await?;
                Output::CodeServers(self.code.list(*worktree).await?)
            }
            Command::ListCodePackages { worktree } => {
                let root = self.worktree_root(*worktree).await?;
                let mut workspace = self.code.packages(*worktree, root, &self.closed).await?;
                for language in &mut workspace.languages {
                    for candidate in &mut language.servers {
                        if let Some(startup) = &candidate.startup {
                            candidate.startup_approved =
                                self.code_approved(startup.clone()).await?;
                        }
                    }
                }
                Output::CodePackages(workspace)
            }
            Command::ReadCodeCatalog => Output::CodeCatalog(self.code.catalog().await?),
            Command::ListCodeInstallations => Output::CodeInstallations(self.code.installations()?),
            Command::CancelCodeInstall { package } => {
                Output::CodeInstallation(self.code.cancel_install(package)?)
            }
            Command::CancelCodeQuery { request } => {
                self.code_queries.cancel(caller, *request);
                Output::CodeQueryCancelled { request: *request }
            }
            Command::ReadCodeRules => Output::CodeRules(self.code.rules_status()?),
            _ => unreachable!("code read expected"),
        };
        let (reply, completion) = tokio::sync::oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    pub(super) async fn change_code(
        &self,
        request: &Request,
        stop: &CancellationToken,
        reporter: Option<&crate::installation::Reporter>,
    ) -> Result<Output, Fault> {
        match &request.command {
            Command::StartCodeServer { startup } => {
                let root = self.worktree_root(startup.worktree).await?;
                self.code
                    .start(startup, &root, stop)
                    .await
                    .map(Output::CodeServer)
            }
            Command::StopCodeServer { startup } => {
                let root = self.worktree_root(startup.worktree).await?;
                self.code.stop(startup, &root).await.map(Output::CodeServer)
            }
            Command::InstallCodeServer { install } => self
                .code
                .install(install, stop, reporter)
                .await
                .map(Output::CodeInstallation),
            Command::UninstallCodeServer { package, version } => {
                self.code.uninstall(package, version).await?;
                Ok(Output::CodeUninstalled {
                    package: package.clone(),
                    version: version.clone(),
                })
            }
            Command::UpdateCodeRules => self.code.update_rules(stop).await.map(Output::CodeRules),
            _ => unreachable!("language lifecycle expected"),
        }
    }
}

pub(super) async fn settings(db: &Connection) -> Result<sailry_protocol::code::Settings, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM code_settings WHERE singleton=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(()).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(|settings| settings.unwrap_or_default())
}

pub(super) async fn initialize(
    db: &mut Connection,
) -> Result<sailry_protocol::code::Settings, Fault> {
    let transaction = db.transaction().await.map_err(storage_error)?;
    if let Some(enabled) =
        super::plugins::services::enabled(&transaction, sailry_protocol::plugin::Service::Language)
            .await?
    {
        enable(&transaction, enabled).await?;
    }
    let settings = settings(&transaction).await?;
    transaction.commit().await.map_err(storage_error)?;
    Ok(settings)
}
pub(super) async fn save(
    db: &Connection,
    value: &sailry_protocol::code::Settings,
    expected: u64,
) -> Result<sailry_protocol::code::Settings, Fault> {
    let current = settings(db).await?;
    if value.revision != expected || current.revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "code settings changed before saving",
        ));
    }
    if value.servers.len() > 64
        || value.languages.len() > 64
        || serde_json::to_vec(value).map_err(storage_error)?.len() > 64 * 1024
    {
        return Err(Fault::invalid("code settings exceed the limit"));
    }
    for (name, server) in &value.servers {
        if name.is_empty()
            || name.len() > 256
            || name.chars().any(char::is_control)
            || server.command.as_ref().is_some_and(|command| {
                command.is_empty() || command.len() > 4096 || command.contains('\0')
            })
            || server.args.as_ref().is_some_and(|args| {
                args.len() > 64
                    || args
                        .iter()
                        .any(|arg| arg.len() > 4096 || arg.contains('\0'))
            })
            || server.environment.as_ref().is_some_and(|values| {
                values.len() > 64
                    || values.iter().any(|(name, value)| {
                        name.is_empty() || name.contains(['=', '\0']) || value.contains('\0')
                    })
            })
        {
            return Err(Fault::invalid("invalid language server override"));
        }
    }
    if value.languages.iter().any(|(language, servers)| {
        language.is_empty()
            || language.len() > 256
            || servers.len() > 16
            || servers.iter().any(|server| {
                server.is_empty() || server.len() > 256 || server.chars().any(char::is_control)
            })
    }) {
        return Err(Fault::invalid("invalid language candidate override"));
    }
    let mut saved = value.clone();
    saved.revision = expected
        .checked_add(1)
        .ok_or_else(|| Fault::invalid("code settings revision is exhausted"))?;
    db.execute("INSERT INTO code_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",[encode(&saved)?]).await.map_err(storage_error)?;
    Ok(saved)
}

pub(super) async fn enable(db: &Connection, enabled: bool) -> Result<(), Fault> {
    let mut settings = settings(db).await?;
    if settings.enabled != enabled {
        let expected = settings.revision;
        settings.enabled = enabled;
        save(db, &settings, expected).await?;
    }
    Ok(())
}

pub(super) fn observe(
    _database: &Database,
    request: &Request,
    result: &sailry_link::Response,
    pool: &std::sync::Arc<crate::code::Pool>,
    runtime: &tokio::runtime::Handle,
) {
    if result.is_err() {
        return;
    }
    let change = match &request.command {
        Command::WriteFile { worktree, path, .. }
        | Command::EditFile { worktree, path, .. }
        | Command::DeleteFile { worktree, path, .. }
        | Command::FinishFileUpload { worktree, path, .. }
        | Command::TrashFile { worktree, path, .. }
        | Command::TrashEntry { worktree, path } => Some((*worktree, vec![path.clone()])),
        Command::RenameEntry {
            worktree, from, to, ..
        }
        | Command::MoveFile {
            worktree, from, to, ..
        }
        | Command::CopyEntry {
            worktree, from, to, ..
        } => Some((*worktree, vec![from.clone(), to.clone()])),
        Command::CopyEntryTo { worktree, to, .. } | Command::MoveEntryTo { worktree, to, .. } => {
            Some((*worktree, vec![to.clone()]))
        }
        // The successful restore validated this worktree against the immutable checkpoint.
        Command::RestoreFileCheckpoint { worktree, .. } => {
            result.as_ref().ok().and_then(|output| {
                if let Output::FileRestored(restored) = output {
                    Some((*worktree, vec![restored.path.clone()]))
                } else {
                    None
                }
            })
        }
        Command::ApplyTurnHunk(selection) => result.as_ref().ok().and_then(|output| match output {
            Output::HunkApplied(applied)
                if selection.operation
                    == sailry_protocol::conversation::checkpoint::hunk::Operation::Reject
                    && applied.index.is_none()
                    && applied.session == selection.session
                    && applied.turn == selection.turn
                    && applied.path == selection.path
                    && applied.hunk == selection.hunk =>
            {
                Some((selection.worktree, vec![applied.path.clone()]))
            }
            _ => None,
        }),
        _ => None,
    };
    if let Some((worktree, paths)) = change {
        let pool = pool.clone();
        runtime.spawn(async move {
            let _ = pool
                .changed(worktree, &paths, &CancellationToken::new())
                .await;
        });
    }
    if let Command::RemoveWorktree { worktree, .. } = request.command {
        let pool = pool.clone();
        runtime.spawn(async move {
            pool.remove(worktree).await;
        });
    }
}

mod management;
#[cfg(test)]
mod tests;

pub(super) async fn refresh(
    db: &Connection,
    pool: &std::sync::Arc<crate::code::Pool>,
    runtime: &tokio::runtime::Handle,
) {
    match settings(db).await {
        Ok(settings) => {
            runtime.spawn(pool.configure(settings));
        }
        Err(error) => eprintln!("language settings refresh failed: {:?}", error.code),
    }
}
