use super::check;
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, NodeId, RequestId};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(crate) struct Queries(Mutex<State>);

#[derive(Default)]
struct State {
    running: BTreeMap<(NodeId, RequestId), CancellationToken>,
    cancelled: BTreeMap<(NodeId, RequestId), Instant>,
}

pub(crate) struct Query {
    owner: Arc<Queries>,
    key: (NodeId, RequestId),
    pub stop: CancellationToken,
}

impl Queries {
    pub(crate) fn begin(
        self: &Arc<Self>,
        caller: NodeId,
        id: RequestId,
        closed: &CancellationToken,
    ) -> Result<Query, Fault> {
        let mut queries = self.0.lock().expect("code query lock is healthy");
        queries
            .cancelled
            .retain(|_, time| time.elapsed() < Duration::from_secs(120));
        if queries.cancelled.remove(&(caller, id)).is_some() {
            return Err(super::cancelled());
        }
        if queries.running.len() >= 128 || queries.running.contains_key(&(caller, id)) {
            return Err(Fault::new(
                ErrorCode::Busy,
                "code query capacity is exhausted",
            ));
        }
        let stop = closed.child_token();
        check(&stop)?;
        queries.running.insert((caller, id), stop.clone());
        Ok(Query {
            owner: self.clone(),
            key: (caller, id),
            stop,
        })
    }

    pub(crate) fn cancel(&self, caller: NodeId, id: RequestId) {
        let mut queries = self.0.lock().expect("code query lock is healthy");
        if let Some(stop) = queries.running.get(&(caller, id)) {
            stop.cancel();
        } else {
            // A cancelled transport observer can finish before deferred admission.
            // Retain a bounded tombstone so that request cannot start afterwards.
            if queries.cancelled.len() >= 128
                && let Some(key) = queries
                    .cancelled
                    .iter()
                    .min_by_key(|(_, time)| **time)
                    .map(|(key, _)| *key)
            {
                queries.cancelled.remove(&key);
            }
            queries.cancelled.insert((caller, id), Instant::now());
        }
    }
}

impl Query {
    pub(crate) async fn run<T>(
        &self,
        query: impl std::future::Future<Output = Result<T, Fault>>,
    ) -> Result<T, Fault> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled,"code query was cancelled")),
            result = tokio::time::timeout(Duration::from_secs(120),query) => {
                result.map_err(|_| Fault::new(ErrorCode::Busy,"code query deadline exceeded"))?
            }
        }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        self.stop.cancel();
        self.owner
            .0
            .lock()
            .expect("code query lock is healthy")
            .running
            .remove(&self.key);
    }
}
