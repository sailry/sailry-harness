//! Scoped resource calls use the ordinary durable request ledger and Agent cancellation.
use super::{Ingress, Job, database::Database, external::Completed};
use futures::FutureExt;
use sailry_link::Response;
use sailry_protocol::*;
use std::{panic::AssertUnwindSafe, sync::Arc};
use tokio::{runtime::Handle, sync::oneshot};

pub(super) fn prepare(database: &Database, request: &Request) -> Result<(), Fault> {
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
            let run = super::agent::visible_run(&database.connection, *session, *turn)?;
            if run.origin.is_some() {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "media turn belongs to another session",
                ));
            }
            (*session, *worktree)
        }
        Command::UseComputer {
            session,
            worktree,
            name,
            arguments,
        } => {
            crate::computer::execute::validate(name, arguments)?;
            (*session, *worktree)
        }
        _ => unreachable!("scoped resource command expected"),
    };
    let session = super::commands::session(&database.connection, session)?;
    if session.worktree != worktree {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "resource session belongs to another worktree",
        ));
    }
    database.worktree_root(worktree)?;
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
        if self.pending >= super::CAPACITY || self.ingress.closed.is_cancelled() {
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
                    Command::UseComputer { .. } => {
                        crate::computer::execute::run(
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
