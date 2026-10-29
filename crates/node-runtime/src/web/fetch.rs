//! Public web reads with consistent paged snapshots.
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
mod render;
#[cfg(test)]
mod tests;

pub(crate) use render::Renderer;

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
        self.fetch_with(caller, id, input, stop, None).await
    }

    pub async fn fetch_with(
        self: &Arc<Self>,
        caller: NodeId,
        id: RequestId,
        input: &web::Request,
        stop: &CancellationToken,
        renderer: Option<&Renderer>,
    ) -> Result<web::Page, Fault> {
        let url = policy::url(&input.url)?;
        let deadline = tokio::time::Instant::now() + self.deadline;
        let query = {
            let mut active = self.active.lock().expect("web fetch lock is healthy");
            if active.contains_key(&(caller, id)) {
                return Err(Fault::new(
                    ErrorCode::Busy,
                    "web fetch request is already running",
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
            result = tokio::time::timeout_at(deadline, self.page(url, input.offset, renderer, deadline)) => {
                result.map_err(|_| Fault::new(ErrorCode::Unavailable, "web fetch deadline exceeded"))?
            }
        }
    }

    async fn page(
        &self,
        url: reqwest::Url,
        offset: usize,
        renderer: Option<&Renderer>,
        deadline: tokio::time::Instant,
    ) -> Result<web::Page, Fault> {
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
            let direct = network::fetch(self, url).await?;
            let loaded = if let Some(renderer) = renderer.filter(|_| render::needed(&direct)) {
                renderer
                    .fetch(
                        self,
                        direct,
                        deadline.saturating_duration_since(tokio::time::Instant::now()),
                    )
                    .await?
            } else {
                direct
            };
            *document = Some(loaded);
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
    pub fn fixture_proxy(&self, host: &str, proxy: hyper_util::client::proxy::matcher::Matcher) {
        self.fixtures
            .lock()
            .expect("web fixture lock is healthy")
            .get_mut(host)
            .expect("web proxy requires an existing fixture")
            .proxy = Some(Arc::new(proxy));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture_dns(&self, host: &str, addresses: Vec<std::net::IpAddr>) {
        self.fixtures
            .lock()
            .expect("web fixture lock is healthy")
            .get_mut(host)
            .expect("web DNS requires an existing fixture")
            .resolved = addresses;
    }

    #[cfg(any(test, feature = "test-support"))]
    fn configure_fixture(
        &self,
        url: &reqwest::Url,
        builder: reqwest::ClientBuilder,
    ) -> reqwest::ClientBuilder {
        let fixtures = self.fixtures.lock().expect("web fixture lock is healthy");
        let Some(fixture) = url.host_str().and_then(|host| fixtures.get(host)) else {
            return builder;
        };
        let mut builder = builder;
        if !fixture.roots.is_empty() {
            builder = builder.tls_certs_only(fixture.roots.clone());
        }
        builder
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
                    proxy: None,
                    roots: Vec::new(),
                },
            );
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
struct Fixture {
    address: std::net::SocketAddr,
    resolved: Vec<std::net::IpAddr>,
    proxy: Option<Arc<hyper_util::client::proxy::matcher::Matcher>>,
    roots: Vec<reqwest::Certificate>,
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
        self.owner
            .cache
            .lock()
            .expect("web cache lock is healthy")
            .trim();
    }
}
