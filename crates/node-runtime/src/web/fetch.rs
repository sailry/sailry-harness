//! Public, credential-free web reads with consistent paged snapshots.
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, NodeId, RequestId, web};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

mod cache;
mod html;
mod network;
mod policy;
#[cfg(test)]
mod tests;

pub(crate) struct Fetcher {
    active: Mutex<BTreeMap<(NodeId, RequestId), CancellationToken>>,
    deadline: Duration,
    cache: Mutex<cache::Cache>,
    #[cfg(any(test, feature = "test-support"))]
    fixtures: Mutex<BTreeMap<String, Fixture>>,
}

impl Default for Fetcher {
    fn default() -> Self {
        Self {
            active: Default::default(),
            deadline: Duration::from_secs(web::TIMEOUT_SECS),
            cache: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            fixtures: Default::default(),
        }
    }
}

impl Fetcher {
    pub async fn fetch(
        self: &Arc<Self>,
        caller: NodeId,
        id: RequestId,
        input: &web::Request,
        stop: &CancellationToken,
    ) -> Result<web::Page, Fault> {
        let url = policy::url(&input.url)?;
        let query = {
            let mut active = self.active.lock().expect("web fetch lock is healthy");
            if active.len() >= 8 || active.contains_key(&(caller, id)) {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "web fetch capacity is exhausted",
                ));
            }
            let stop = stop.child_token();
            active.insert((caller, id), stop.clone());
            Query {
                owner: self.clone(),
                key: (caller, id),
                stop,
            }
        };
        tokio::select! {
            biased;
            _ = query.stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "web fetch was cancelled")),
            result = tokio::time::timeout(self.deadline, self.page(url, input.offset)) => {
                result.map_err(|_| Fault::new(ErrorCode::Unavailable, "web fetch deadline exceeded"))?
            }
        }
    }

    async fn page(&self, url: reqwest::Url, offset: usize) -> Result<web::Page, Fault> {
        let slot = self
            .cache
            .lock()
            .expect("web cache lock is healthy")
            .slot(url.as_str(), offset)?;
        let mut document = slot.lock().await;
        if document.as_ref().is_some_and(cache::Document::expired) {
            *document = None;
        }
        if document.is_none() {
            if offset != 0 {
                return Err(cache::restart());
            }
            *document = Some(network::fetch(self, url).await?);
        }
        document
            .as_ref()
            .expect("web document is loaded")
            .page(offset)
    }

    pub fn cancel(&self, caller: NodeId, id: RequestId) {
        if let Some(stop) = self
            .active
            .lock()
            .expect("web fetch lock is healthy")
            .get(&(caller, id))
        {
            stop.cancel();
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture(&self, host: &str, address: std::net::SocketAddr) {
        assert!(address.ip().is_loopback(), "web fixture must use loopback");
        assert!(
            host.ends_with(".test"),
            "web fixture must use a test hostname"
        );
        self.fixtures
            .lock()
            .expect("web fixture lock is healthy")
            .insert(
                host.into(),
                Fixture {
                    address,
                    resolved: vec![std::net::Ipv4Addr::new(8, 8, 8, 8).into()],
                },
            );
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
struct Fixture {
    address: std::net::SocketAddr,
    resolved: Vec<std::net::IpAddr>,
}

struct Query {
    owner: Arc<Fetcher>,
    key: (NodeId, RequestId),
    stop: CancellationToken,
}

impl Drop for Query {
    fn drop(&mut self) {
        self.stop.cancel();
        self.owner
            .active
            .lock()
            .expect("web fetch lock is healthy")
            .remove(&self.key);
    }
}
