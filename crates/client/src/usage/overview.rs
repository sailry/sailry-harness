//! Combine reports without merging execution identities or creating a usage ledger.
use super::*;
use sailry_protocol::{
    NodeId,
    usage::{Group, Metrics, Point},
};
use std::collections::BTreeMap;
use tokio::task::JoinSet;

mod summary;

#[derive(Clone, serde::Serialize)]
pub struct Source {
    pub node: NodeId,
    pub view: View,
}

#[derive(Clone, serde::Serialize)]
pub struct ScopedGroup {
    pub node: NodeId,
    pub group: Group,
}

#[derive(Clone, serde::Serialize)]
pub struct Summary {
    pub totals: Metrics,
    pub days: Vec<Point>,
    pub recent: Vec<Point>,
    pub groups: Vec<ScopedGroup>,
    pub requests: sailry_protocol::usage::Page,
}

#[derive(Clone, Default, serde::Serialize)]
pub struct Overview {
    pub sources: Vec<Source>,
    /// Includes the latest known reports; inspect complete before treating them as current.
    pub summary: Option<Arc<Summary>>,
    pub complete: bool,
    pub error: Option<Fault>,
}

impl Overview {
    pub fn refreshing(&self) -> bool {
        self.sources.iter().any(|source| source.view.refreshing)
    }
}

struct Closing(watch::Sender<Overview>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| {
            view.complete = false;
            for source in &mut view.sources {
                source.view.connected = false;
                source.view.refreshing = false;
            }
        });
    }
}

/// Observe each unique execution Node once. Dropping this future cancels its observers.
pub async fn watch_overview(
    clients: Vec<Arc<Client>>,
    query: Query,
    updates: watch::Sender<Overview>,
    stop: CancellationToken,
    mut refresh: mpsc::Receiver<()>,
) -> Result<(), Fault> {
    let _closing = Closing(updates.clone());
    let clients: BTreeMap<_, _> = clients
        .into_iter()
        .map(|client| (client.target(), client))
        .collect();
    if clients.is_empty() {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "usage overview requires an execution Node",
        ));
    }
    let mut sources: BTreeMap<_, _> = clients
        .keys()
        .map(|node| (*node, View::default()))
        .collect();
    for source in &updates.borrow().sources {
        if let Some(view) = sources.get_mut(&source.node)
            && source
                .view
                .report
                .as_ref()
                .is_some_and(|report| report.node == source.node && report.query == query)
        {
            view.report = source.view.report.clone();
        }
    }
    let (changes, mut changed) = mpsc::channel(clients.len());
    let mut workers = JoinSet::new();
    let mut requests = Vec::new();
    for (node, client) in clients {
        let (sender, mut receiver) = watch::channel(sources[&node].clone());
        let (request, queued) = mpsc::channel(1);
        requests.push(request);
        let query = query.clone();
        let stop = stop.clone();
        let changes = changes.clone();
        workers.spawn(async move {
            let worker = client.watch_usage(query, sender, stop.clone(), queued);
            tokio::pin!(worker);
            loop {
                let finished = tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    result = &mut worker => {
                        let mut view = receiver.borrow_and_update().clone();
                        if let Err(error) = result { view.error = Some(error); }
                        let _ = changes.send((node, view)).await;
                        break;
                    },
                    result = receiver.changed() => result.is_err(),
                };
                if finished {
                    break;
                }
                let view = receiver.borrow_and_update().clone();
                if changes.send((node, view)).await.is_err() {
                    break;
                }
            }
        });
    }
    drop(changes);
    publish(&sources, &updates);
    let mut requests_open = true;
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            _ = updates.closed() => return Ok(()),
            request = refresh.recv(), if requests_open => {
                if request.is_none() { requests_open = false; continue; }
                for request in &requests { let _ = request.try_send(()); }
            },
            change = changed.recv() => {
                let Some((node, view)) = change else { return Ok(()) };
                sources.insert(node, view);
                publish(&sources, &updates);
            },
        }
    }
}

fn publish(sources: &BTreeMap<NodeId, View>, updates: &watch::Sender<Overview>) {
    let sources: Vec<_> = sources
        .iter()
        .map(|(node, view)| Source {
            node: *node,
            view: view.clone(),
        })
        .collect();
    let complete = sources.iter().all(|source| {
        source.view.connected
            && !source.view.refreshing
            && source.view.error.is_none()
            && source.view.report.is_some()
    });
    let (summary, error) = match summary::combine(&sources) {
        Ok(summary) => (summary.map(Arc::new), None),
        Err(error) => (None, Some(error)),
    };
    let _ = updates.send(Overview {
        sources,
        summary,
        complete: complete && error.is_none(),
        error,
    });
}
