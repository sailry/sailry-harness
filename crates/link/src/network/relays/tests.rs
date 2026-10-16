//! Real local relay handshakes and transport checks with isolated fixture certificates.
use super::*;
use crate::{Admission, Local, PeerStore, Stream, Subscription};
use iroh::tls::CaTlsConfig;
use iroh_relay::server::{CertConfig, Server, ServerConfig, TlsConfig, testing};
use sailry_protocol::{NodeId, Request, StreamId, Topic};
use std::{collections::BTreeSet, net::Ipv4Addr, sync::Mutex as StdMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Peers(StdMutex<BTreeSet<NodeId>>);

impl PeerStore for Peers {
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>> {
        Box::pin(async { Ok(self.0.lock().unwrap().iter().copied().collect()) })
    }
    fn addresses(&self) -> Pending<'_, Result<Vec<EndpointAddr>, Fault>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn remember(&self, _: EndpointAddr) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async { Ok(()) })
    }
    fn set_trust(&self, peer: NodeId, trusted: bool) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async move {
            let mut peers = self.0.lock().unwrap();
            if trusted {
                peers.insert(peer);
            } else {
                peers.remove(&peer);
            }
            Ok(())
        })
    }
}

struct Echo(StreamId);
impl Handler for Echo {
    fn open(&self, _: NodeId, resource: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        Box::pin(async move {
            if resource != self.0 {
                return Err(Fault::new(
                    ErrorCode::NotFound,
                    "fixture resource not found",
                ));
            }
            let (client, service) = tokio::io::duplex(1024);
            tokio::spawn(async move {
                let (mut read, mut write) = tokio::io::split(service);
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
            Ok(Box::new(client) as Stream)
        })
    }
    fn dispatch(&self, _: NodeId, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async { Err(frame::invalid("fixture has no command service")) })
    }
    fn subscribe(&self, _: NodeId, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async { Err(frame::invalid("fixture has no event service")) })
    }
}

struct RelayFixture {
    server: Server,
    selection: RelaySelection,
    tls: CaTlsConfig,
}

impl RelayFixture {
    async fn start() -> Self {
        Self::start_many(1).await.pop().unwrap()
    }

    async fn start_many(count: usize) -> Vec<Self> {
        let (certificates, config) = testing::self_signed_tls_certs_and_config();
        let tls = CaTlsConfig::custom_roots(certificates);
        let mut fixtures = Vec::new();
        for _ in 0..count {
            let mut relay = iroh_relay::server::RelayConfig::new((Ipv4Addr::LOCALHOST, 0));
            relay.tls = Some(TlsConfig::new(
                (Ipv4Addr::LOCALHOST, 0),
                CertConfig::Manual {
                    server_config: config.clone(),
                },
            ));
            let mut config = ServerConfig::default();
            config.relay = Some(relay);
            let server = Server::spawn(config).await.unwrap();
            let selection =
                RelaySelection::Custom(vec![format!("https://{}", server.https_addr().unwrap())]);
            fixtures.push(Self {
                server,
                selection,
                tls: tls.clone(),
            });
        }
        fixtures
    }

    async fn link(&self, handler: Arc<dyn Handler>) -> Link {
        let endpoint = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Custom(self.selection.map().unwrap()))
            .ca_tls_config(self.tls.clone())
            .alpns(vec![ALPN.to_vec(), pairing::ALPN.to_vec()])
            .clear_ip_transports()
            .bind()
            .await
            .unwrap();
        Link::from_endpoint(
            endpoint,
            Some(self.selection.clone()),
            handler,
            Arc::new(Peers::default()),
        )
        .await
        .unwrap()
    }
}

#[test]
fn validates_urls() {
    RelaySelection::Default.validate().unwrap();
    RelaySelection::Custom(vec!["https://relay.example:8443".into()])
        .validate()
        .unwrap();
    for urls in [
        vec![],
        vec!["https://relay.example".into(); 9],
        vec!["http://relay.example".into()],
        vec!["https://name:secret@relay.example".into()],
        vec!["https://relay.example/?token=value".into()],
        vec!["https://relay.example/#fragment".into()],
        vec!["not a URL".into()],
    ] {
        assert_eq!(
            RelaySelection::Custom(urls).validate().unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }
}

#[tokio::test]
async fn handshakes_and_preserves_local_and_remote_access() {
    let mut fixtures = RelayFixture::start_many(2).await;
    let relay = fixtures.pop().unwrap();
    let previous = fixtures.pop().unwrap();
    let handler = Arc::new(Echo(StreamId::new()));
    let first = previous.link(handler.clone()).await;
    let second = relay.link(handler.clone()).await;
    let link = first.handle();
    let id = link.address().id;
    link.select_relays(relay.selection.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(link.address().id, id);
    assert_eq!(
        link.relay_selection().unwrap(),
        Some(relay.selection.clone())
    );
    let mut invitation = link.invite().unwrap();
    let ticket: serde_json::Value = serde_json::from_str(invitation.ticket()).unwrap();
    assert_eq!(ticket["version"], 1);
    let advertised = link.address().relay_urls().cloned().collect::<Vec<_>>();
    assert_eq!(advertised, relay.selection.map().unwrap().urls::<Vec<_>>());
    assert!(invitation.ticket().contains(&advertised[0].to_string()));
    second.handle().pair(invitation.ticket()).await.unwrap();
    invitation.paired().await.unwrap();
    let target = NodeId(*id.as_bytes());
    let caller = NodeId(*second.handle().address().id.as_bytes());
    for transport in [
        Arc::new(Local::new(target, caller, handler.clone())) as Arc<dyn Transport>,
        second.handle().remote(link.address()),
    ] {
        let mut stream = transport.open(handler.0).await.unwrap();
        stream.write_all(b"shared relay transport").await.unwrap();
        stream.shutdown().await.unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"shared relay transport");
    }
    drop(invitation);
    first.close().await.unwrap();
    second.close().await.unwrap();
    drop(relay.server);
}

#[tokio::test]
async fn cancelled_probe_restores_the_previous_network() {
    let relay = RelayFixture::start().await;
    let link = relay.link(Arc::new(Echo(StreamId::new()))).await;
    link.handle().online().await.unwrap();
    let stop = CancellationToken::new();
    let token = stop.clone();
    let handle = link.handle();
    let probe = tokio::spawn(async move {
        handle
            .select_relays(
                RelaySelection::Custom(vec!["https://127.0.0.1:1".into()]),
                token,
            )
            .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    stop.cancel();
    assert!(probe.await.unwrap().is_err());
    assert_eq!(
        link.handle().relay_selection().unwrap(),
        Some(relay.selection.clone())
    );
    link.handle().online().await.unwrap();
    assert_eq!(
        link.handle().address().relay_urls().collect::<Vec<_>>(),
        relay
            .selection
            .map()
            .unwrap()
            .urls::<Vec<_>>()
            .iter()
            .collect::<Vec<_>>()
    );
    link.close().await.unwrap();
}

#[tokio::test]
async fn rollback_survives_a_dropped_caller() {
    let relay = RelayFixture::start().await;
    let link = relay.link(Arc::new(Echo(StreamId::new()))).await;
    link.handle().online().await.unwrap();
    let stop = CancellationToken::new();
    let token = stop.clone();
    let handle = link.handle();
    let probe = tokio::spawn(async move {
        handle
            .select_relays(
                RelaySelection::Custom(vec!["https://127.0.0.1:1".into()]),
                token,
            )
            .await
    });
    // A failed relay never becomes the home relay. Observe the owned update
    // instead of waiting for an unreachable server to be advertised.
    tokio::time::timeout(Duration::from_secs(3), async {
        while link.handle.core.relay_update.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    probe.abort();
    stop.cancel();
    let update = link.handle.core.relay_update.lock().await;
    assert_eq!(
        link.handle().relay_selection().unwrap(),
        Some(relay.selection.clone())
    );
    drop(update);
    tokio::time::timeout(
        Duration::from_secs(3),
        link.handle
            .core
            .relay_ready(&relay.selection.map().unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    link.close().await.unwrap();
}

#[tokio::test]
async fn rejects_untrusted_tls_without_committing() {
    let previous = RelayFixture::start().await;
    let rejected = RelayFixture::start().await;
    let link = previous.link(Arc::new(Echo(StreamId::new()))).await;
    link.handle().online().await.unwrap();
    assert!(
        link.handle()
            .select_relays(rejected.selection, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(
        link.handle().relay_selection().unwrap(),
        Some(previous.selection)
    );
    link.handle().online().await.unwrap();
    link.close().await.unwrap();
}
