//! Real local relay handshakes and transport checks with isolated fixture certificates.
use super::*;
use crate::{Admission, Local, PeerStore, Stream, Subscription};
use iroh::tls::CaTlsConfig;
use iroh_relay::server::{CertConfig, Server, ServerConfig, TlsConfig, testing};
use sailry_protocol::{NodeId, Request, StreamId, Topic};
use std::{collections::BTreeSet, net::Ipv4Addr, sync::Mutex as StdMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Peers(
    StdMutex<BTreeSet<NodeId>>,
    StdMutex<HashMap<NodeId, EndpointAddr>>,
);

impl PeerStore for Peers {
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>> {
        Box::pin(async { Ok(self.0.lock().unwrap().iter().copied().collect()) })
    }
    fn addresses(&self) -> Pending<'_, Result<Vec<EndpointAddr>, Fault>> {
        Box::pin(async { Ok(self.1.lock().unwrap().values().cloned().collect()) })
    }
    fn remember(&self, address: EndpointAddr) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async move {
            self.1
                .lock()
                .unwrap()
                .insert(NodeId(*address.id.as_bytes()), address);
            Ok(())
        })
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
        self.link_with_ip(handler, false).await
    }

    async fn link_with_ip(&self, handler: Arc<dyn Handler>, direct: bool) -> Link {
        let builder = Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Custom(self.selection.map().unwrap()))
            .ca_tls_config(self.tls.clone())
            .alpns(vec![ALPN.to_vec(), pairing::ALPN.to_vec()]);
        let endpoint = if direct {
            builder
        } else {
            builder.clear_ip_transports()
        }
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

#[tokio::test]
async fn observes_selected_relay_and_ip_paths() {
    use crate::{ConnectionInfo, Route};
    for direct in [false, true] {
        let relay = RelayFixture::start().await;
        let handler = Arc::new(Echo(StreamId::new()));
        let first = relay.link_with_ip(handler.clone(), direct).await;
        let second = relay.link_with_ip(handler, direct).await;
        first.handle().online().await.unwrap();
        second.handle().online().await.unwrap();
        // The authenticated handshake is seeded only with the real local relay.
        let mut invitation = first.handle().invite().unwrap();
        second.handle().pair(invitation.ticket()).await.unwrap();
        invitation.paired().await.unwrap();
        let mut address = first.handle().address();
        address.addrs.retain(|address| address.is_relay());
        let connection = second
            .handle
            .core
            .connection(&address, false)
            .await
            .unwrap();
        let updates = second
            .handle
            .core
            .outgoing
            .lock()
            .await
            .get(&NodeId(*address.id.as_bytes()))
            .unwrap()
            .cached
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .telemetry
            .clone();
        let (sender, mut observed) = watch::channel(ConnectionInfo::default());
        let remote = second.handle().remote(address.clone());
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let task = tokio::spawn(async move {
            remote.watch_connection(sender, cancellation).await;
        });
        let expected = if direct { Route::P2p } else { Route::Relay };
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if observed.borrow_and_update().route == Some(expected) {
                    break;
                }
                observed.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let paths = connection.paths();
        let selected = paths.iter().find(|path| path.is_selected()).unwrap();
        assert_eq!(selected.is_ip(), direct);
        assert_eq!(selected.is_relay(), !direct);
        assert!(observed.borrow().rtt_ms.is_some());
        // Reading or observing again reuses this connection's one telemetry owner.
        second.handle().inspect(&address).await.unwrap();
        let cached = second.handle.core.outgoing.lock().await;
        assert!(
            updates.same_channel(
                &cached
                    .get(&NodeId(*address.id.as_bytes()))
                    .unwrap()
                    .cached
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .telemetry
            )
        );
        drop(cached);
        stop.cancel();
        task.await.unwrap();
        assert_eq!(*observed.borrow(), ConnectionInfo::default());
        first.close().await.unwrap();
        second.close().await.unwrap();
    }
}

#[tokio::test]
async fn reconnects_existing_handles_after_host_switch() {
    let mut fixtures = RelayFixture::start_many(2).await;
    let next = fixtures.pop().unwrap();
    let previous = fixtures.pop().unwrap();
    let handler = Arc::new(Echo(StreamId::new()));
    let host = previous.link(handler.clone()).await;
    let controller = previous.link(handler.clone()).await;
    host.handle().online().await.unwrap();
    controller.handle().online().await.unwrap();
    let mut invitation = host.handle().invite().unwrap();
    assert_eq!(
        controller
            .handle()
            .pair_on_network(invitation.ticket(), &next.selection)
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let address = controller
        .handle()
        .pair_on_network(invitation.ticket(), &previous.selection)
        .await
        .unwrap();
    invitation.paired().await.unwrap();
    let remote = controller.handle().remote(address.clone());
    drop(remote.open(handler.0).await.unwrap());
    let id = host.handle().address().id;
    host.handle()
        .select_relays(next.selection.clone(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(host.handle().address().id, id);
    let candidate = controller
        .handle()
        .candidate_address(&address, &next.selection)
        .unwrap();
    assert_eq!(candidate.id, id);
    assert_eq!(
        candidate.ip_addrs().collect::<Vec<_>>(),
        address.ip_addrs().collect::<Vec<_>>()
    );
    controller
        .handle()
        .update_peer(candidate.clone())
        .await
        .unwrap();
    assert_eq!(
        controller.handle().relay_selection().unwrap(),
        Some(previous.selection)
    );
    assert_eq!(
        controller.handle().peers().await.unwrap(),
        vec![candidate.clone()]
    );
    controller.handle().disconnect(NodeId(*id.as_bytes())).await;
    let mut stream = remote.open(handler.0).await.unwrap();
    stream
        .write_all(b"same identity, new host relay")
        .await
        .unwrap();
    stream.shutdown().await.unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"same identity, new host relay");
    assert_eq!(
        controller
            .handle
            .core
            .outgoing
            .lock()
            .await
            .get(&NodeId(*id.as_bytes()))
            .unwrap()
            .cached
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .address,
        candidate
    );
    controller.close().await.unwrap();
    host.close().await.unwrap();
}

mod routes {
    use super::*;

    struct Commands {
        calls: StdMutex<Vec<sailry_protocol::RequestId>>,
        release: Option<Arc<tokio::sync::Notify>>,
    }

    impl Handler for Commands {
        fn dispatch(&self, _: NodeId, request: Request) -> Pending<'_, Result<Admission, Fault>> {
            Box::pin(async move {
                let release = {
                    let mut calls = self.calls.lock().unwrap();
                    calls.push(request.id);
                    if calls.len() == 1 {
                        self.release.clone()
                    } else {
                        None
                    }
                };
                let (reply, completion) = tokio::sync::oneshot::channel();
                tokio::spawn(async move {
                    if let Some(release) = release {
                        release.notified().await;
                    }
                    let _ = reply.send(Ok(sailry_protocol::Output::Plugins(vec![])));
                });
                Ok(Admission {
                    receipt: sailry_protocol::Receipt {
                        id: request.id,
                        durable: true,
                    },
                    completion,
                })
            })
        }

        fn subscribe(
            &self,
            _: NodeId,
            _: Topic,
        ) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
            Box::pin(async { Err(frame::invalid("fixture has no event service")) })
        }
    }

    #[tokio::test]
    async fn refreshes_cached_routes_without_replaying_admitted_commands() {
        let directory = tempfile::tempdir().unwrap();
        let identity = Identity::open(directory.path()).unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let release = Arc::new(tokio::sync::Notify::new());
        let previous_handler = Arc::new(Commands {
            calls: StdMutex::new(vec![]),
            release: Some(release.clone()),
        });
        let next_handler = Arc::new(Commands {
            calls: StdMutex::new(vec![]),
            release: None,
        });
        let previous = Link::bind(
            &identity,
            NetworkScope::default(),
            previous_handler.clone(),
            Arc::new(Peers::default()),
        )
        .await
        .unwrap();
        let next = Link::bind(
            &identity,
            NetworkScope::default(),
            next_handler.clone(),
            Arc::new(Peers::default()),
        )
        .await
        .unwrap();
        let control = controller.handle();
        let peer = identity.id();
        control.set_trust(peer, true).await.unwrap();
        previous
            .handle()
            .set_trust(NodeId(*control.address().id.as_bytes()), true)
            .await
            .unwrap();
        next.handle()
            .set_trust(NodeId(*control.address().id.as_bytes()), true)
            .await
            .unwrap();
        let previous_address = previous.handle().address();
        let next_address = next.handle().address();
        assert_ne!(previous_address.addrs, next_address.addrs);
        let original = Request::new(
            peer,
            sailry_protocol::Command::RegisterProject {
                name: "Previous route".into(),
                path: "/fixture".into(),
            },
        );
        let admitted = control
            .remote(previous_address.clone())
            .dispatch(original.clone())
            .await
            .unwrap();
        assert!(admitted.receipt.durable);
        assert_eq!(admitted.receipt.id, original.id);
        let connection = control
            .core
            .connection(&previous_address, true)
            .await
            .unwrap();
        assert!(connection.close_reason().is_none());
        let unaddressed = control
            .core
            .connection(&EndpointAddr::new(previous_address.id), true)
            .await
            .unwrap();
        assert_eq!(unaddressed.stable_id(), connection.stable_id());
        let request = Request::new(
            peer,
            sailry_protocol::Command::RegisterProject {
                name: "Current route".into(),
                path: "/fixture".into(),
            },
        );
        let current = control
            .remote(next_address.clone())
            .dispatch(request.clone())
            .await
            .unwrap();
        let slot = control
            .core
            .outgoing
            .lock()
            .await
            .get(&peer)
            .unwrap()
            .clone();
        {
            let cached = slot.cached.lock().unwrap();
            let cached = cached.as_ref().unwrap();
            assert_ne!(cached.connection.stable_id(), connection.stable_id());
            assert_eq!(cached.address, next_address);
        }
        // Transport addresses are hints for one identity, not endpoint incarnations.
        let recorded = || {
            let mut calls = previous_handler.calls.lock().unwrap().clone();
            calls.extend(next_handler.calls.lock().unwrap().iter().copied());
            calls
        };
        assert_eq!(recorded(), [original.id, request.id]);
        assert!(
            matches!(current.completion.await.unwrap().unwrap(), sailry_protocol::Output::Plugins(items) if items.is_empty())
        );
        let error = tokio::time::timeout(Duration::from_secs(3), admitted.completion)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::OutcomeUnknown);
        release.notify_one();
        assert_eq!(recorded(), [original.id, request.id]);
        previous.close().await.unwrap();
        next.close().await.unwrap();
        controller.close().await.unwrap();
    }
}
