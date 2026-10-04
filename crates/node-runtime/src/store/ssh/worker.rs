use super::super::{Job, external::Completed};
use futures::FutureExt;
use sailry_link::{CancellationToken, Response};
use sailry_protocol::*;
use std::{collections::BTreeMap, panic::AssertUnwindSafe};
use tokio::{
    runtime::Handle,
    sync::{mpsc, oneshot},
};

pub(in crate::store) struct Execution {
    pub caller: NodeId,
    pub request: Request,
    pub operation: Option<crate::ssh::Operation>,
    pub reply: oneshot::Sender<Response>,
}

pub(in crate::store) struct Worker {
    transfers: std::sync::Arc<crate::files::transfers::Transfers>,
    pub pending: usize,
    pub link: Option<sailry_link::LinkHandle>,
    progress: BTreeMap<(NodeId, RequestId), tokio::sync::watch::Receiver<ssh::InstallProgress>>,
    runtime: Handle,
    completed: mpsc::Sender<Job>,
    closed: CancellationToken,
    terminals: std::sync::Arc<crate::terminal::Terminals>,
    active: BTreeMap<(NodeId, RequestId), (CancellationToken, Option<plugin::Context>)>,
}

impl Worker {
    pub fn new(
        transfers: std::sync::Arc<crate::files::transfers::Transfers>,
        runtime: Handle,
        completed: mpsc::Sender<Job>,
        closed: CancellationToken,
        terminals: std::sync::Arc<crate::terminal::Terminals>,
    ) -> Self {
        Self {
            transfers,
            pending: 0,
            link: None,
            progress: BTreeMap::new(),
            runtime,
            completed,
            closed,
            terminals,
            active: BTreeMap::new(),
        }
    }

    pub fn submit(&mut self, execution: Execution) -> Result<(), Box<Execution>> {
        if let Command::ReadHostInstall { request } = execution.request.command {
            let scope_matches = execution.request.plugin.is_none()
                || self
                    .active
                    .get(&(execution.caller, request))
                    .is_some_and(|(_, context)| *context == execution.request.plugin);
            let result = self
                .progress
                .get(&(execution.caller, request))
                .filter(|_| scope_matches)
                .map(|progress| Output::HostInstallProgress(progress.borrow().clone()))
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "Host installation is not active"));
            self.pending += 1;
            let completed = self.completed.clone();
            self.runtime.spawn(async move {
                let _ = completed
                    .send(Job::Ssh(Box::new(Completed {
                        caller: execution.caller,
                        request: execution.request,
                        result,
                        reply: execution.reply,
                    })))
                    .await;
            });
            return Ok(());
        }
        let cancellation = if let Command::CancelSsh { request } = execution.request.command {
            Some(match self.active.get(&(execution.caller, request)) {
                Some((_, context))
                    if execution.request.plugin.is_some()
                        && execution.request.plugin != *context =>
                {
                    Err(Fault::new(
                        ErrorCode::PermissionDenied,
                        "connection request belongs to another plugin scope",
                    ))
                }
                Some((stop, _)) => {
                    stop.cancel();
                    Ok(Output::SshCancelled { request })
                }
                None => Ok(Output::SshCancelled { request }),
            })
        } else {
            None
        };
        if cancellation.is_none() && (self.active.len() >= 8 || self.closed.is_cancelled()) {
            return Err(Box::new(execution));
        }
        let stop = CancellationToken::new();
        if cancellation.is_none() {
            self.active.insert(
                (execution.caller, execution.request.id),
                (stop.clone(), execution.request.plugin.clone()),
            );
        }
        let deployment = if matches!(execution.request.command, Command::InstallHost { .. }) {
            let Some(link) = self.link.clone() else {
                self.active
                    .remove(&(execution.caller, execution.request.id));
                return Err(Box::new(execution));
            };
            let (sender, receiver) = tokio::sync::watch::channel(ssh::InstallProgress::Connecting);
            self.progress
                .insert((execution.caller, execution.request.id), receiver);
            Some((link, sender))
        } else {
            None
        };
        self.pending += 1;
        let completed = self.completed.clone();
        let closed = self.closed.clone();
        let terminals = self.terminals.clone();
        let transfers = self.transfers.clone();
        self.runtime.spawn(async move {
            let result = if let Some(output) = cancellation {
                output
            } else if let Command::CloseSshTerminal { terminal } = execution.request.command {
                tokio::task::spawn_blocking(move || terminals.close_ssh(terminal))
                    .await
                    .unwrap_or_else(|_| {
                        Err(Fault::new(
                            ErrorCode::OutcomeUnknown,
                            "SSH terminal close outcome is unavailable",
                        ))
                    })
                    .map(Output::Terminal)
            } else {
                AssertUnwindSafe(crate::ssh::execute(
                    execution.operation.expect("SSH operation"),
                    &stop,
                    &closed,
                    terminals,
                    execution.caller,
                    deployment,
                    transfers,
                ))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "SSH operation failed; inspect the remote target before retrying",
                    ))
                })
                .map(Output::SshOutcome)
            };
            let _ = completed
                .send(Job::Ssh(Box::new(Completed {
                    caller: execution.caller,
                    request: execution.request,
                    result,
                    reply: execution.reply,
                })))
                .await;
        });
        Ok(())
    }

    pub fn finished(&mut self, caller: NodeId, request: RequestId) {
        self.active.remove(&(caller, request));
        self.progress.remove(&(caller, request));
        self.pending -= 1;
    }
}
