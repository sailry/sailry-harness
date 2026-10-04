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
    pub operation: Option<crate::databases::Operation>,
    pub reply: oneshot::Sender<Response>,
}

pub(in crate::store) struct Worker {
    pub pending: usize,
    runtime: Handle,
    completed: mpsc::Sender<Job>,
    closed: CancellationToken,
    active: BTreeMap<(NodeId, RequestId), (CancellationToken, Option<plugin::Context>)>,
}

impl Worker {
    pub fn new(runtime: Handle, completed: mpsc::Sender<Job>, closed: CancellationToken) -> Self {
        Self {
            pending: 0,
            runtime,
            completed,
            closed,
            active: BTreeMap::new(),
        }
    }

    pub fn submit(&mut self, execution: Execution) -> Result<(), Box<Execution>> {
        let cancellation = if let Command::CancelDatabase { request } = execution.request.command {
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
                    Ok(Output::DatabaseCancelled { request })
                }
                None => Ok(Output::DatabaseCancelled { request }),
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
        self.pending += 1;
        let completed = self.completed.clone();
        let closed = self.closed.clone();
        self.runtime.spawn(async move {
            let result = if let Some(output) = cancellation {
                output
            } else {
                AssertUnwindSafe(crate::databases::execute(
                    execution.operation.expect("database operation"),
                    &stop,
                    &closed,
                ))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "database operation failed; inspect the remote target before retrying",
                    ))
                })
                .map(Output::DatabaseOutcome)
            };
            let _ = completed
                .send(Job::Database(Box::new(Completed {
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
        self.pending -= 1;
    }
}
