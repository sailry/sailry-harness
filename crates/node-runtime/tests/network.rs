use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, NodeId, Output, network::Selection};

#[tokio::test]
async fn reads_same_endpoint_and_rejects_direct_changes() {
    for remote in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let node = Node::start_empty(root.path().join("node")).await.unwrap();
        let controller = Link::controller(root.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let caller = NodeId(*controller.handle().address().id.as_bytes());
        node.link().set_trust(caller, true).await.unwrap();
        controller
            .handle()
            .set_trust(node.id(), true)
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::NetworkSettings(settings) = client
            .execute(client.prepare(Command::ReadNetworkSettings))
            .await
            .unwrap()
        else {
            panic!("network settings expected");
        };
        assert_eq!(settings.revision, 0);
        assert_eq!(settings.selection, None);
        assert_eq!(settings.effective, None);
        let address: sailry_link::EndpointAddr = serde_json::from_str(&settings.address).unwrap();
        assert_eq!(NodeId(*address.id.as_bytes()), node.id());
        let request = client.prepare(Command::SaveNetworkSettings {
            expected_revision: 0,
            selection: Selection::Iroh,
        });
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let error = admission.completion.await.unwrap().unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(client.execute(request).await.unwrap_err(), error);
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadNetworkSettings))
                .await
                .unwrap(),
            Output::NetworkSettings(settings)
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

struct Relays {
    servers: Vec<iroh_relay::server::Server>,
    roots: sailry_link::CaTlsConfig,
    urls: Vec<String>,
}

impl Relays {
    async fn start() -> Self {
        use iroh_relay::server::{CertConfig, Server, ServerConfig, TlsConfig, testing};
        use std::net::Ipv4Addr;
        let (certificates, config) = testing::self_signed_tls_certs_and_config();
        let roots = sailry_link::CaTlsConfig::custom_roots(certificates);
        let mut servers = Vec::new();
        let mut urls = Vec::new();
        for _ in 0..2 {
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
            urls.push(format!("https://{}", server.https_addr().unwrap()));
            servers.push(server);
        }
        Self {
            servers,
            roots,
            urls,
        }
    }
}

async fn settings(client: &Client) -> sailry_protocol::network::Settings {
    match client
        .execute(client.prepare(Command::ReadNetworkSettings))
        .await
        .unwrap()
    {
        Output::NetworkSettings(settings) => settings,
        _ => panic!("network settings expected"),
    }
}

#[tokio::test]
async fn switches_host_and_recovers_receipt() {
    use sailry_protocol::RequestOutcome;
    use std::time::Duration;
    for remote in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let relays = Relays::start().await;
        let previous = Selection::Custom {
            relays: vec![relays.urls[0].clone()],
        };
        let desired = Selection::Custom {
            relays: vec![relays.urls[1].clone()],
        };
        let scope = || NetworkScope::CustomRelays(vec![relays.urls[0].clone()]);
        let path = root.path().join("host");
        let node = Node::start_empty_with_network_roots(&path, scope(), relays.roots.clone())
            .await
            .unwrap();
        node.link().online().await.unwrap();
        let id = node.id();
        let controller = Node::start_empty_with_network_roots(
            root.path().join("controller"),
            scope(),
            relays.roots.clone(),
        )
        .await
        .unwrap();
        controller.link().online().await.unwrap();
        let mut invitation = node.link().invite().unwrap();
        let mut address = controller.link().pair(invitation.ticket()).await.unwrap();
        invitation.paired().await.unwrap();
        address.addrs.retain(|address| address.is_relay());
        let client = Client::new(if remote {
            controller.link().remote(address.clone())
        } else {
            node.local()
        });
        let before = settings(&client).await;
        assert_eq!(before.revision, 0);
        assert_eq!(before.effective, Some(previous.clone()));
        let request = client.prepare(Command::SaveNetworkSettings {
            expected_revision: before.revision,
            selection: desired.clone(),
        });
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::NotAdmitted
        );
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        assert_eq!(admission.receipt.id, request.id);
        if remote {
            // Deliberately lose only the controller's completion receiver. The admitted
            // Node worker must finish and store its result without a second Save.
            drop(admission.completion);
            tokio::time::timeout(Duration::from_secs(10), async {
                while node.link().relay_selection().unwrap()
                    != Some(sailry_link::RelaySelection::Custom(vec![
                        relays.urls[1].clone(),
                    ]))
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let candidate = controller
                .link()
                .candidate_address(
                    &address,
                    &sailry_link::RelaySelection::Custom(vec![relays.urls[1].clone()]),
                )
                .unwrap();
            controller.link().update_peer(candidate).await.unwrap();
        } else {
            assert!(matches!(
                admission.completion.await.unwrap().unwrap(),
                Output::NetworkSettings(_)
            ));
        }
        let completed = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match client.outcome(&request).await.unwrap() {
                    RequestOutcome::Completed(result) => break *result,
                    RequestOutcome::Admitted => tokio::task::yield_now().await,
                    other => panic!("admitted relay change lost its outcome: {other:?}"),
                }
            }
        })
        .await
        .unwrap()
        .unwrap();
        let Output::NetworkSettings(saved) = completed.clone() else {
            panic!("saved network expected");
        };
        assert_eq!(saved.revision, 1);
        assert_eq!(saved.selection, Some(desired.clone()));
        assert_eq!(saved.effective, Some(desired.clone()));
        assert_eq!(node.id(), id);
        assert_eq!(
            controller.link().relay_selection().unwrap(),
            Some(sailry_link::RelaySelection::Custom(vec![
                relays.urls[0].clone()
            ]))
        );
        let actual: sailry_link::EndpointAddr = serde_json::from_str(&saved.address).unwrap();
        assert_eq!(NodeId(*actual.id.as_bytes()), id);
        if remote {
            controller.link().update_peer(actual).await.unwrap();
        }
        assert_eq!(client.execute(request.clone()).await.unwrap(), completed);
        assert_eq!(settings(&client).await.revision, 1);
        let stale = client.prepare(Command::SaveNetworkSettings {
            expected_revision: 0,
            selection: previous,
        });
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(settings(&client).await.revision, 1);
        drop(invitation);
        node.shutdown().await.unwrap();
        let node = Node::start_empty_with_network_roots(&path, scope(), relays.roots.clone())
            .await
            .unwrap();
        node.link().online().await.unwrap();
        assert_eq!(node.id(), id);
        let client = if remote {
            controller.link().disconnect(id).await;
            controller
                .link()
                .update_peer(node.link().address())
                .await
                .unwrap();
            client
        } else {
            Client::new(node.local())
        };
        let restored = settings(&client).await;
        assert_eq!(restored.revision, 1);
        assert_eq!(restored.selection, Some(desired.clone()));
        assert_eq!(restored.effective, Some(desired));
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Ok(completed)))
        );
        node.shutdown().await.unwrap();
        controller.shutdown().await.unwrap();
        assert_eq!(relays.servers.len(), 2);
    }
}
