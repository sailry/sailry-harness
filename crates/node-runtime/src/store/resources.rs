//! Scoped resource calls use the ordinary durable request ledger and Agent cancellation.
use super::{Ingress, Job, database::Database, external::Completed};
use futures::FutureExt;
use sailry_link::Response;
use sailry_protocol::*;
use std::{panic::AssertUnwindSafe, sync::Arc};
use tokio::{runtime::Handle, sync::oneshot};

pub(super) async fn prepare(database: &Database, request: &Request) -> Result<(), Fault> {
    if let Command::CheckPluginMcp { package, server } = &request.command {
        return super::plugins::mcp::prepare(&database.connection, package, server)
            .await
            .map(|_| ());
    }
    if let Command::CloseTerminal { worktree, .. } = &request.command {
        database.worktree_root(*worktree).await?;
        return Ok(());
    }
    if matches!(
        request.command,
        Command::ExportBackup { .. }
            | Command::RestoreBackup { .. }
            | Command::RestoreBackupFile { .. }
    ) {
        return super::backup::prepare(database, request).await;
    }
    if matches!(
        request.command,
        Command::InstallLocalModel { .. } | Command::DeleteLocalModel { .. }
    ) {
        return super::local_models::approvals::check(&database.connection, request).await;
    }
    if matches!(
        request.command,
        Command::StartCodeServer { .. }
            | Command::StopCodeServer { .. }
            | Command::InstallCodeServer { .. }
            | Command::UninstallCodeServer { .. }
            | Command::UpdateCodeRules
    ) {
        return super::code::prepare(database, request).await;
    }
    let (session, worktree) = match &request.command {
        Command::UseBrowser {
            session,
            worktree,
            action,
        } => {
            crate::browser::validate(action)?;
            (*session, *worktree)
        }
        Command::UseExternalBrowser {
            session,
            worktree,
            action,
            arguments,
        } => {
            crate::external_browser::validate(*action, arguments)?;
            (*session, *worktree)
        }
        Command::UseMedia {
            turn,
            session,
            worktree,
            action,
        } => {
            crate::agent::media::validate(action)?;
            let run = super::agent::visible_run(&database.connection, *session, *turn).await?;
            if run.origin.is_some() {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "media turn belongs to another session",
                ));
            }
            (*session, *worktree)
        }
        _ => unreachable!("scoped resource command expected"),
    };
    let session = super::commands::session(&database.connection, session).await?;
    if session.worktree != worktree {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "resource session belongs to another worktree",
        ));
    }
    database.worktree_root(worktree).await?;
    Ok(())
}

pub(super) fn turn(request: &Request) -> Option<TurnId> {
    request
        .plugin
        .as_ref()
        .and_then(|context| context.turn)
        .or({
            if let Command::UseMedia { turn, .. } = request.command {
                Some(turn)
            } else {
                None
            }
        })
}

pub(super) struct Execution {
    pub caller: NodeId,
    pub request: Request,
    pub reply: oneshot::Sender<Response>,
}

pub(super) struct Worker {
    pub pending: usize,
    ingress: Arc<Ingress>,
    runtime: Handle,
}

impl Worker {
    pub fn new(ingress: Arc<Ingress>) -> Self {
        Self {
            pending: 0,
            ingress,
            runtime: Handle::current(),
        }
    }

    pub fn submit(&mut self, execution: Execution) -> Result<(), Box<Execution>> {
        if self.ingress.closed.is_cancelled() {
            return Err(Box::new(execution));
        }
        let stop = if let Some(turn) = turn(&execution.request) {
            let Some(stop) = self.ingress.agents.begin_operation(turn) else {
                return Err(Box::new(execution));
            };
            stop.child_token()
        } else {
            self.ingress.closed.child_token()
        };
        self.pending += 1;
        let ingress = self.ingress.clone();
        self.runtime.spawn(async move {
            let operation = async {
                match &execution.request.command {
                    Command::CheckPluginMcp { package, server } => {
                        crate::agent::check_mcp(&ingress, package, server, &stop)
                            .await
                            .map(Output::PluginMcpChecked)
                    }
                    Command::CloseTerminal { worktree, terminal } => {
                        let terminals = ingress.terminals.clone();
                        let worktree = *worktree;
                        let terminal = *terminal;
                        tokio::task::spawn_blocking(move || terminals.close(worktree, terminal))
                            .await
                            .map_err(|_| {
                                Fault::new(
                                    ErrorCode::OutcomeUnknown,
                                    "terminal close outcome is unavailable",
                                )
                            })?
                            .map(Output::Terminal)
                    }
                    Command::ExportBackup { .. }
                    | Command::RestoreBackup { .. }
                    | Command::RestoreBackupFile { .. } => {
                        ingress
                            .change_backup(execution.caller, &execution.request, &stop)
                            .await
                    }
                    Command::StartCodeServer { .. }
                    | Command::StopCodeServer { .. }
                    | Command::InstallCodeServer { .. }
                    | Command::UninstallCodeServer { .. }
                    | Command::UpdateCodeRules => {
                        ingress.change_code(&execution.request, &stop, None).await
                    }
                    Command::UseBrowser { .. } => {
                        crate::browser::execute(
                            &ingress,
                            execution.caller,
                            &execution.request,
                            stop,
                        )
                        .await
                    }
                    Command::UseExternalBrowser { .. } => {
                        crate::external_browser::Call::new(
                            ingress.clone(),
                            execution.caller,
                            execution.request.clone(),
                            stop,
                        )
                        .execute()
                        .await
                    }
                    Command::UseMedia { .. } => {
                        crate::agent::media::run(
                            &ingress,
                            execution.caller,
                            &execution.request,
                            stop,
                        )
                        .await
                    }
                    _ => unreachable!("scoped resource command expected"),
                }
            };
            let result = AssertUnwindSafe(operation)
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "resource result is unknown; verify before retrying",
                    ))
                });
            let _ = ingress
                .sender
                .send(Job::Resources(Box::new(Completed {
                    caller: execution.caller,
                    request: execution.request,
                    result,
                    reply: execution.reply,
                })))
                .await;
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
