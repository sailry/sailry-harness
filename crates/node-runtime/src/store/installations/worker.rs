//! Reuse bounded admission and per-resource preparation order, not another inbox.
use crate::{
    installation::Reporter,
    store::{Ingress, Job, external::Completed, installations::Execution},
};
use futures::FutureExt;
use sailry_link::{CancellationToken, Response};
use sailry_protocol::{
    ErrorCode, Fault,
    installation::{Phase, Resource},
    models::Kind,
};
use std::{collections::BTreeMap, panic::AssertUnwindSafe, sync::Arc};
use tokio::{runtime::Handle, sync::oneshot};

pub(in crate::store) struct Worker {
    pub pending: usize,
    ingress: Arc<Ingress>,
    runtime: Handle,
    preceding: BTreeMap<Lane, oneshot::Receiver<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Lane {
    Plugin(String),
    Model(Kind),
}

fn lane(resource: &Resource) -> Option<Lane> {
    match resource {
        Resource::Plugin { name } => Some(Lane::Plugin(name.clone())),
        // Model services serialize mutation within a family, including distinct model IDs.
        Resource::Model { kind, .. } => Some(Lane::Model(*kind)),
        Resource::Language { .. } => None,
    }
}

impl Worker {
    pub fn new(ingress: Arc<Ingress>) -> Self {
        Self {
            pending: 0,
            ingress,
            runtime: Handle::current(),
            preceding: BTreeMap::new(),
        }
    }

    pub fn submit<F, Future>(
        &mut self,
        execution: Execution,
        resource: Resource,
        prepare: F,
    ) -> Result<(), Box<Execution>>
    where
        F: FnOnce(Reporter, CancellationToken) -> Future + Send + 'static,
        Future: std::future::Future<Output = Response> + Send + 'static,
    {
        let Some(lane) = lane(&resource) else {
            return Err(Box::new(execution));
        };
        let reporter: Reporter = if execution.observe {
            self.ingress
                .installations
                .begin(execution.caller, &execution.request, resource.clone())
        } else {
            Arc::new(|_, _| {})
        };
        if self.pending >= 8 || self.ingress.closed.is_cancelled() {
            return Err(Box::new(execution));
        }
        let (stop, operation) = if let Some(turn) = execution
            .request
            .plugin
            .as_ref()
            .and_then(|entry| entry.turn)
        {
            let stop = if execution.operation {
                self.ingress.agents.begin_operation(turn)
            } else {
                self.ingress.agents.python_stop(turn)
            };
            let Some(stop) = stop else {
                return Err(Box::new(execution));
            };
            (stop.child_token(), execution.operation.then_some(turn))
        } else {
            (self.ingress.closed.child_token(), None)
        };
        retain(&mut self.preceding);
        let (release, waiting) = oneshot::channel();
        let preceding = self.preceding.insert(lane, waiting);
        self.pending += 1;
        let ingress = self.ingress.clone();
        self.runtime.spawn(async move {
            let closing = ingress.closed.clone();
            let preparing = stop.clone();
            let watcher = tokio::spawn(async move {
                closing.cancelled().await;
                preparing.cancel();
            });
            let preparation = async {
                wait(preceding, &stop).await?;
                let output = prepare(reporter.clone(), stop.clone()).await?;
                check(&stop)?;
                // The task stays active until the Store records this output and its business changes.
                reporter(Phase::Installing, None);
                Ok(output)
            };
            let result = AssertUnwindSafe(preparation)
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "installation preparation failed; inspect the resource before retrying",
                    ))
                });
            watcher.abort();
            let sent = ingress
                .sender
                .send(Job::Installed {
                    completed: Box::new(Completed {
                        caller: execution.caller,
                        request: execution.request,
                        result,
                        reply: execution.reply,
                    }),
                    operation,
                    release,
                })
                .await;
            if sent.is_err()
                && let Some(turn) = operation
            {
                ingress.agents.finish_operation(turn);
            }
        });
        Ok(())
    }
}

async fn wait(
    preceding: Option<oneshot::Receiver<()>>,
    stop: &CancellationToken,
) -> Result<(), Fault> {
    // Even cancellation must wait for a predecessor's durable completion.
    if let Some(preceding) = preceding {
        let _ = preceding.await;
    }
    check(stop)
}

fn retain(preceding: &mut BTreeMap<Lane, oneshot::Receiver<()>>) {
    preceding.retain(|_, waiting| {
        matches!(waiting.try_recv(), Err(oneshot::error::TryRecvError::Empty))
    });
}

fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        Err(Fault::new(
            ErrorCode::Cancelled,
            "installation preparation cancelled",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
