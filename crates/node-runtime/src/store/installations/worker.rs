//! Reuse bounded admission and per-resource preparation order, not another inbox.
use crate::{
    installation::Reporter,
    store::{Ingress, Job, external::Completed, mutations::Mutation},
};
use futures::FutureExt;
use sailry_link::{CancellationToken, Response};
use sailry_protocol::{
    ErrorCode, Fault,
    installation::{Phase, Resource},
};
use std::{collections::BTreeMap, panic::AssertUnwindSafe, sync::Arc};
use tokio::{runtime::Handle, sync::oneshot};

pub(in crate::store) struct Worker {
    pub pending: usize,
    ingress: Arc<Ingress>,
    runtime: Handle,
    preceding: BTreeMap<Resource, oneshot::Receiver<()>>,
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
        mutation: Mutation,
        resource: Resource,
        prepare: F,
    ) -> Result<(), Box<Mutation>>
    where
        F: FnOnce(Reporter, CancellationToken) -> Future + Send + 'static,
        Future: std::future::Future<Output = Response> + Send + 'static,
    {
        let reporter =
            self.ingress
                .installations
                .begin(mutation.caller, &mutation.request, resource.clone());
        if self.pending >= 8 || self.ingress.closed.is_cancelled() {
            return Err(Box::new(mutation));
        }
        let stop = if let Some(turn) = mutation
            .request
            .plugin
            .as_ref()
            .and_then(|entry| entry.turn)
        {
            let Some(stop) = self.ingress.agents.python_stop(turn) else {
                return Err(Box::new(mutation));
            };
            stop.child_token()
        } else {
            self.ingress.closed.child_token()
        };
        retain(&mut self.preceding);
        let (release, waiting) = oneshot::channel();
        let preceding = self.preceding.insert(resource, waiting);
        self.pending += 1;
        let ingress = self.ingress.clone();
        self.runtime.spawn(async move {
            let closing = ingress.closed.clone();
            let preparing = stop.clone();
            let watcher = tokio::spawn(async move {
                closing.cancelled().await;
                preparing.cancel();
            });
            let operation = async {
                wait(preceding, &stop).await?;
                let output = prepare(reporter.clone(), stop.clone()).await?;
                check(&stop)?;
                // The task stays active until the Store records this output and its business changes.
                reporter(Phase::Installing, None);
                Ok(output)
            };
            let result = AssertUnwindSafe(operation)
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "installation preparation failed; inspect the resource before retrying",
                    ))
                });
            watcher.abort();
            let _ = ingress
                .sender
                .send(Job::Installed {
                    completed: Box::new(Completed {
                        caller: mutation.caller,
                        request: mutation.request,
                        result,
                        reply: mutation.reply,
                    }),
                    release,
                })
                .await;
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

fn retain(preceding: &mut BTreeMap<Resource, oneshot::Receiver<()>>) {
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
