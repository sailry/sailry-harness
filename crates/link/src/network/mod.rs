mod frame;
mod pairing;
mod remote;
mod server;
mod stream;

use std::{
    collections::{BTreeSet, HashMap},
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::Duration,
};

use iroh::{
    Endpoint, EndpointAddr, RelayMode,
    endpoint::{Connection, QuicTransportConfig, presets},
};
use sailry_protocol::{ErrorCode, Fault, NodeId};
use tokio::{
    sync::{Mutex, mpsc, oneshot, watch},
    task::{JoinHandle, JoinSet},
};

use crate::{CancellationToken, Handler, Identity, Pending, Transport};
use frame::network;

const ALPN: &[u8] = b"sailry/node/1";
const MAX_CONNECTIONS: usize = 16;
const MAX_TASKS: usize = 64;
const MAX_STREAMS: usize = 64;
const STREAM_WINDOW: u32 = 512 * 1024;
const CONNECTION_WINDOW: u32 = 4 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
type Work = Pending<'static, ()>;
pub use pairing::Invitation;

pub trait PeerStore: Send + Sync {
    /// Release controller-owned storage after the Link worker has stopped.
    /// Injected Node stores retain their own lifecycle owner.
    fn close(&self) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async { Ok(()) })
    }
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>>;
    fn addresses(&self) -> Pending<'_, Result<Vec<EndpointAddr>, Fault>>;
    fn remember(&self, address: EndpointAddr) -> Pending<'_, Result<(), Fault>>;
    fn set_trust(&self, peer: NodeId, trusted: bool) -> Pending<'_, Result<(), Fault>>;
}

#[derive(Clone)]
pub enum NetworkScope {
    Direct(SocketAddr),
    Internet,
    CustomRelays(Vec<String>),
}

impl Default for NetworkScope {
    fn default() -> Self {
        Self::Direct(([127, 0, 0, 1], 0).into())
    }
}

struct Core {
    name: RwLock<Option<String>>,
    endpoint: Endpoint,
    handler: Arc<dyn Handler>,
    store: Arc<dyn PeerStore>,
    trusted: RwLock<BTreeSet<NodeId>>,
    outgoing: Mutex<HashMap<NodeId, CachedConnection>>,
    stop: CancellationToken,
    work: mpsc::Sender<Work>,
    trust_update: Mutex<()>,
    trust_changed: watch::Sender<()>,
    invitation: std::sync::Mutex<Option<pairing::Offer>>,
    auxiliary: Arc<tokio::sync::Semaphore>,
}

struct CachedConnection {
    address: EndpointAddr,
    connection: Connection,
}

pub struct Link {
    handle: LinkHandle,
    worker: JoinHandle<Result<(), Fault>>,
}

pub struct PeerInfo {
    pub execution: bool,
    pub name: Option<String>,
    pub platform: String,
    pub latency: Duration,
}

#[derive(Clone)]
pub struct LinkHandle {
    core: Arc<Core>,
}

impl Link {
    pub async fn bind(
        identity: &Identity,
        scope: NetworkScope,
        handler: Arc<dyn Handler>,
        store: Arc<dyn PeerStore>,
    ) -> Result<Self, Fault> {
        let trusted = store.peers().await?.into_iter().collect();
        let builder = match scope {
            NetworkScope::Internet => Endpoint::builder(presets::N0),
            NetworkScope::CustomRelays(urls) => {
                let relays = custom_relays(&urls)?;
                Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Custom(relays))
            }
            NetworkScope::Direct(address) => Endpoint::builder(presets::Minimal)
                .relay_mode(RelayMode::Disabled)
                .clear_address_lookup()
                .clear_ip_transports()
                .bind_addr(address)
                .map_err(network)?,
        };
        let endpoint = builder
            .secret_key(identity.0.clone())
            .transport_config(
                QuicTransportConfig::builder()
                    .max_concurrent_bidi_streams((MAX_STREAMS as u32).into())
                    .max_concurrent_uni_streams(0u32.into())
                    .stream_receive_window(STREAM_WINDOW.into())
                    .receive_window(CONNECTION_WINDOW.into())
                    .send_window(CONNECTION_WINDOW.into())
                    .build(),
            )
            .alpns(vec![ALPN.to_vec(), pairing::ALPN.to_vec()])
            .bind()
            .await
            .map_err(network)?;
        let (work, receiver) = mpsc::channel(MAX_TASKS);
        let core = Arc::new(Core {
            name: RwLock::new(None),
            endpoint,
            handler,
            store,
            trusted: RwLock::new(trusted),
            outgoing: Mutex::new(HashMap::new()),
            stop: CancellationToken::new(),
            work,
            trust_update: Mutex::new(()),
            trust_changed: watch::channel(()).0,
            invitation: std::sync::Mutex::new(None),
            auxiliary: Arc::new(tokio::sync::Semaphore::new(32)),
        });
        let worker = tokio::spawn(drive(core.clone(), receiver));
        Ok(Self {
            handle: LinkHandle { core },
            worker,
        })
    }

    pub fn handle(&self) -> LinkHandle {
        self.handle.clone()
    }

    pub async fn close(mut self) -> Result<(), Fault> {
        self.handle.core.stop.cancel();
        (&mut self.worker).await.map_err(network)?
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.handle.core.stop.cancel();
    }
}

impl LinkHandle {
    /// The platform supplies a display name; endpoint keys remain the identity.
    pub fn set_name(&self, name: String) -> Result<(), Fault> {
        let name = name.trim();
        if name.chars().count() > 128 || name.chars().any(char::is_control) {
            return Err(frame::invalid("invalid device name"));
        }
        *self.core.name.write().map_err(network)? = (!name.is_empty()).then(|| name.to_owned());
        Ok(())
    }

    /// Probe an authenticated peer without attempting a business command on a controller.
    pub async fn inspect(&self, address: &EndpointAddr) -> Result<PeerInfo, Fault> {
        let connection = self.core.connection(address).await?;
        let started = std::time::Instant::now();
        let (mut send, mut recv) = self
            .core
            .wait(connection.open_bi())
            .await?
            .map_err(frame::network)?;
        self.core
            .wait(frame::write(&mut send, &frame::Call::Inspect))
            .await??;
        send.finish().map_err(frame::network)?;
        match self
            .core
            .wait(frame::read::<frame::Reply>(&mut recv))
            .await??
        {
            frame::Reply::Endpoint {
                execution,
                name,
                platform,
            } => Ok(PeerInfo {
                execution,
                name,
                platform,
                latency: started.elapsed(),
            }),
            frame::Reply::Rejected(error) => Err(error),
            _ => Err(frame::invalid("endpoint inspection expected")),
        }
    }

    /// Mobile lifecycle adapters notify the existing endpoint, not another runtime.
    pub async fn network_changed(&self) -> Result<(), Fault> {
        self.core.wait(self.core.endpoint.network_change()).await
    }
    /// Changes to trusted peers or their last authenticated addresses.
    pub fn peer_changes(&self) -> watch::Receiver<()> {
        self.core.trust_changed.subscribe()
    }
    pub async fn peers(&self) -> Result<Vec<EndpointAddr>, Fault> {
        self.core.store.addresses().await
    }

    async fn remember(&self, address: EndpointAddr) -> Result<(), Fault> {
        let _update = self.core.trust_update.lock().await;
        let peer = NodeId(*address.id.as_bytes());
        if !self.core.trusted(peer) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "peer is not paired",
            ));
        }
        self.core.store.remember(address).await?;
        self.core.trust_changed.send_replace(());
        Ok(())
    }
    /// Wait before issuing an Internet invitation so its ticket includes the home relay.
    /// Direct-only callers should not call this method.
    pub async fn online(&self) -> Result<(), Fault> {
        self.core
            .wait(async {
                self.core.endpoint.online().await;
                Ok::<_, Fault>(())
            })
            .await?
    }

    pub fn address(&self) -> EndpointAddr {
        self.core.endpoint.addr()
    }

    pub fn remote(&self, target: EndpointAddr) -> Arc<dyn Transport> {
        Arc::new(remote::Remote {
            core: self.core.clone(),
            address: target,
        })
    }

    /// Invoked by the local pairing owner, never exposed as a remotely callable command.
    pub async fn set_trust(&self, peer: NodeId, trusted: bool) -> Result<(), Fault> {
        let core = self.core.clone();
        let (reply, response) = oneshot::channel();
        self.core.spawn(Box::pin(async move {
            let result = async {
                let _update = core.trust_update.lock().await;
                core.store.set_trust(peer, trusted).await?;
                {
                    let mut peers = core.trusted.write().map_err(network)?;
                    if trusted {
                        peers.insert(peer);
                    } else {
                        peers.remove(&peer);
                    }
                }
                core.trust_changed.send_replace(());
                if !trusted && let Some(connection) = core.outgoing.lock().await.remove(&peer) {
                    connection.connection.close(1u32.into(), b"peer revoked");
                }
                Ok(())
            }
            .await;
            let _ = reply.send(result);
        }))?;
        response.await.map_err(network)?
    }

    pub async fn disconnect(&self, peer: NodeId) {
        if let Some(connection) = self.core.outgoing.lock().await.remove(&peer) {
            connection
                .connection
                .close(0u32.into(), b"client disconnected");
        }
    }
}

fn custom_relays(urls: &[String]) -> Result<iroh::RelayMap, Fault> {
    if urls.is_empty() || urls.len() > 8 {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "expected one to eight relay URLs",
        ));
    }
    for value in urls {
        let url = reqwest::Url::parse(value)
            .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid relay URL"))?;
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "relay URLs require HTTPS without credentials",
            ));
        }
    }
    iroh::RelayMap::try_from_iter(urls.iter().map(String::as_str))
        .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid relay URL"))
}

impl Core {
    fn trusted(&self, peer: NodeId) -> bool {
        self.trusted.read().is_ok_and(|peers| peers.contains(&peer))
    }

    fn spawn(&self, work: Work) -> Result<(), Fault> {
        if self.stop.is_cancelled() {
            return Err(network("closed"));
        }
        self.work
            .try_send(work)
            .map_err(|_| Fault::new(ErrorCode::Busy, "Link task capacity exhausted"))
    }

    async fn connection(&self, target: &EndpointAddr) -> Result<Connection, Fault> {
        if self.stop.is_cancelled() {
            return Err(network("closed"));
        }
        let id = NodeId(*target.id.as_bytes());
        if !self.trusted(id) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "peer is not paired",
            ));
        }
        // One outgoing connection owner per peer; reconnection never replays a request itself.
        let mut outgoing = self.outgoing.lock().await;
        outgoing.retain(|_, cached| cached.connection.close_reason().is_none());
        if let Some(cached) = outgoing.get(&id) {
            if cached.address == *target {
                return Ok(cached.connection.clone());
            }
            cached
                .connection
                .close(0u32.into(), b"peer address changed");
            outgoing.remove(&id);
        }
        if outgoing.len() >= MAX_CONNECTIONS {
            return Err(Fault::new(
                ErrorCode::Busy,
                "Link connection capacity exhausted",
            ));
        }
        let connection = self
            .wait(self.endpoint.connect(target.clone(), ALPN))
            .await?
            .map_err(network)?;
        if !self.trusted(id) {
            connection.close(1u32.into(), b"peer revoked");
            return Err(Fault::new(ErrorCode::PermissionDenied, "peer was revoked"));
        }
        outgoing.insert(
            id,
            CachedConnection {
                address: target.clone(),
                connection: connection.clone(),
            },
        );
        Ok(connection)
    }

    async fn wait<T>(&self, future: impl std::future::Future<Output = T>) -> Result<T, Fault> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(network("closed")),
            result = tokio::time::timeout(IO_TIMEOUT, future) => result.map_err(network),
        }
    }
}

async fn drive(core: Arc<Core>, mut receiver: mpsc::Receiver<Work>) -> Result<(), Fault> {
    let mut work = JoinSet::new();
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = core.stop.cancelled() => break,
            _ = work.join_next(), if !work.is_empty() => {},
            _ = connections.join_next(), if !connections.is_empty() => {},
            Some(task) = receiver.recv(), if work.len() < MAX_TASKS => { work.spawn(task); },
            incoming = core.endpoint.accept(), if connections.len() < MAX_CONNECTIONS => {
                let Some(incoming) = incoming else { break; };
                let core = core.clone();
                connections.spawn(async move {
                    if let Ok(Ok(connection)) = core.wait(async move { incoming.await }).await {
                        let peer = NodeId(*connection.remote_id().as_bytes());
                        if connection.alpn() == pairing::ALPN { pairing::serve(core, connection, peer).await; }
                        else if core.trusted(peer) { server::serve(core, connection, peer).await; }
                        else { connection.close(1u32.into(), b"peer not paired"); }
                    }
                });
            }
        }
    }
    receiver.close();
    work.shutdown().await;
    connections.shutdown().await;
    core.endpoint.close().await;
    core.store.close().await
}
