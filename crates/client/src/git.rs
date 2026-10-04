//! Shared, read-only Git requests for one controller. Completed results are not cached.
use sailry_link::Transport;
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Request};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::{Semaphore, watch};

type Response = Result<Output, Fault>;

#[derive(Default)]
pub struct Reads {
    nodes: Mutex<BTreeMap<NodeId, Arc<Node>>>,
}

struct Node {
    pending: Mutex<Vec<Weak<Flight>>>,
    capacity: Semaphore,
}

struct Flight {
    request: Request,
    result: watch::Receiver<Option<Response>>,
}

impl Reads {
    pub(crate) async fn execute(
        &self,
        transport: Arc<dyn Transport>,
        request: Request,
    ) -> Response {
        let node = self
            .nodes
            .lock()
            .unwrap()
            .entry(request.target)
            .or_insert_with(|| {
                Arc::new(Node {
                    pending: Default::default(),
                    // Queue distinct reads so views do not compete for the Node's worker slots.
                    capacity: Semaphore::new(1),
                })
            })
            .clone();
        let (flight, start) = {
            let mut pending = node.pending.lock().unwrap();
            pending.retain(|flight| flight.strong_count() > 0);
            let existing = pending.iter().filter_map(Weak::upgrade).find(|flight| {
                flight.request.command == request.command
                    && flight.request.plugin == request.plugin
                    && flight.request.version == request.version
            });
            if let Some(flight) = existing {
                (flight, None)
            } else {
                let (sender, result) = watch::channel(None);
                let flight = Arc::new(Flight {
                    request: request.clone(),
                    result,
                });
                pending.push(Arc::downgrade(&flight));
                (flight, Some(sender))
            }
        };
        if let Some(sender) = start {
            let weak = Arc::downgrade(&flight);
            tokio::spawn(async move {
                let result = tokio::select! {
                    biased;
                    _ = sender.closed() => None,
                    result = read(&node, &*transport, request) => Some(result),
                };
                node.pending
                    .lock()
                    .unwrap()
                    .retain(|pending| !pending.ptr_eq(&weak));
                if let Some(result) = result {
                    sender.send_replace(Some(result));
                }
            });
        }
        let mut receiver = flight.result.clone();
        loop {
            if let Some(result) = receiver.borrow_and_update().clone() {
                return result;
            }
            receiver
                .changed()
                .await
                .map_err(|_| Fault::new(ErrorCode::Unavailable, "Git read stopped"))?;
        }
    }

    // A post-mutation refresh must never join an inspection started before the write.
    pub(crate) fn invalidate(&self, target: NodeId) {
        if let Some(node) = self.nodes.lock().unwrap().get(&target) {
            node.pending.lock().unwrap().clear();
        }
    }
}

async fn read(node: &Node, transport: &dyn Transport, request: Request) -> Response {
    let mut delay = Duration::from_millis(250);
    loop {
        let result = {
            let _permit = node
                .capacity
                .acquire()
                .await
                .map_err(|_| Fault::new(ErrorCode::Unavailable, "Git reader closed"))?;
            super::execute(transport, request.clone()).await
        };
        if !matches!(&result, Err(error) if matches!(error.code, ErrorCode::Busy | ErrorCode::Unavailable))
        {
            return result;
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(5));
    }
}

pub(crate) fn is_read(command: &Command) -> bool {
    matches!(
        command,
        Command::InspectGit { .. }
            | Command::ListGitBranches { .. }
            | Command::ListWorktrees { .. }
            | Command::ReadGitDiff { .. }
            | Command::ReadGitLog { .. }
            | Command::ReadGitCommit { .. }
            | Command::ReadGitStash { .. }
            | Command::ReadGitOutput { .. }
            | Command::ResolveGitRevision { .. }
    )
}

#[cfg(test)]
mod tests;
