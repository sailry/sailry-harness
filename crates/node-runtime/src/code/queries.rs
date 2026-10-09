use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, NodeId, RequestId};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
pub(crate) struct Queries(Mutex<BTreeMap<(NodeId, RequestId), CancellationToken>>);
pub(crate) struct Query {
    owner: Arc<Queries>,
    key: (NodeId, RequestId),
    pub stop: CancellationToken,
}

impl Queries {
    pub fn begin(
        self: &Arc<Self>,
        caller: NodeId,
        id: RequestId,
        closed: &CancellationToken,
    ) -> Result<Query, Fault> {
        let mut queries = self.0.lock().expect("code query lock is healthy");
        if queries.len() >= 128 {
            return Err(Fault::new(
                ErrorCode::Busy,
                "code query capacity is exhausted",
            ));
        }
        if queries.contains_key(&(caller, id)) {
            return Err(Fault::new(
                ErrorCode::Busy,
                "this code query is already running",
            ));
        }
        let stop = closed.child_token();
        queries.insert((caller, id), stop.clone());
        Ok(Query {
            owner: self.clone(),
            key: (caller, id),
            stop,
        })
    }
    pub fn cancel(&self, caller: NodeId, id: RequestId) {
        if let Some(stop) = self
            .0
            .lock()
            .expect("code query lock is healthy")
            .get(&(caller, id))
        {
            stop.cancel();
        }
    }
}
impl Query {
    pub async fn run(
        &self,
        query: impl std::future::Future<Output = Result<sailry_protocol::code::Result, Fault>>,
    ) -> Result<sailry_protocol::code::Result, Fault> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "language query was cancelled")),
            result = tokio::time::timeout(std::time::Duration::from_secs(20), query) => {
                result.map_err(|_| Fault::new(ErrorCode::Unavailable, "language query deadline exceeded"))?
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
            .remove(&self.key);
    }
}

#[cfg(test)]
mod tests;
