use super::{Relays, settings};
use sailry_client::Client;
use sailry_link::NetworkScope;
use sailry_node_runtime::Node;
use sailry_protocol::{
    Command, ErrorCode, NodeId, Output,
    network::{Catalog, Listing, Preset, Selection},
    plugin,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[allow(
    dead_code,
    reason = "Shared HTTP fixture also supports provider-specific responses"
)]
#[path = "../discovery_support/mod.rs"]
mod http;

fn context() -> plugin::Context {
    plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: plugin::Reference {
            name: "uninstalled".into(),
            settings_revision: 1,
            digest: "a".repeat(64),
        },
        worktree: None,
        session: None,
    }
}

#[tokio::test]
async fn reads_from_execution_node_without_changes() {
    for remote in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let relays = Relays::start().await;
        let selection = Selection::Custom {
            relays: relays.urls.clone(),
        };
        let catalog = Catalog {
            version: 1,
            presets: vec![Preset {
                id: "fixture".into(),
                label: plugin::desktop::Navigation {
                    label: "Fixture".into(),
                    icon: None,
                    locales: Default::default(),
                },
                selection: selection.clone(),
            }],
        };
        let fail = Arc::new(AtomicBool::new(false));
        let failed = fail.clone();
        let value = serde_json::to_value(&catalog).unwrap();
        let server = http::Server::start(sailry_protocol::conversation::ModelApi::Responses, move |_| {
            if failed.load(Ordering::SeqCst) {
                http::Reply::Raw("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into())
            } else {
                http::Reply::Json(value.clone())
            }
        }).await;
        let node = Node::start_empty_with_network_roots(
            root.path().join("node"),
            NetworkScope::default(),
            relays.roots.clone(),
        )
        .await
        .unwrap();
        node.set_network_presets_fixture(&server.endpoint).unwrap();
        // Only the addressed Node trusts the probe fixture's certificate.
        let controller = Node::start_empty(root.path().join("controller"))
            .await
            .unwrap();
        let address = controller
            .link()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.link().remote(address)
        } else {
            node.local()
        });
        let before = settings(&client).await;
        let node_peers = node.link().peers().await.unwrap();
        let controller_peers = controller.link().peers().await.unwrap();
        let commands = [
            Command::ListNetworkPresets { refresh: false },
            Command::ListNetworkPresets { refresh: true },
            Command::ProbeNetwork {
                selection: selection.clone(),
            },
        ];
        for command in &commands {
            let mut request = client.prepare(command.clone());
            request.target = NodeId::new();
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::WrongTarget
            );
            assert_eq!(
                client
                    .execute(client.prepare(command.clone()).with_plugin(context()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        assert!(server.requests.lock().unwrap().is_empty());
        assert_eq!(
            client
                .execute(client.prepare(commands[0].clone()))
                .await
                .unwrap(),
            Output::NetworkPresets(Listing::default())
        );
        assert!(server.requests.lock().unwrap().is_empty());
        let request = client.prepare(commands[1].clone());
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert_eq!(admission.receipt.id, request.id);
        assert!(!admission.receipt.durable);
        let loaded = Listing {
            presets: catalog.presets.clone(),
            unavailable: false,
        };
        assert_eq!(
            admission.completion.await.unwrap().unwrap(),
            Output::NetworkPresets(loaded.clone())
        );
        assert_eq!(
            client
                .execute(client.prepare(commands[0].clone()))
                .await
                .unwrap(),
            Output::NetworkPresets(loaded.clone())
        );
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        let request = client.prepare(commands[2].clone());
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert_eq!(admission.receipt.id, request.id);
        assert!(!admission.receipt.durable);
        let Output::NetworkLatency(latency) = admission.completion.await.unwrap().unwrap() else {
            panic!("network latency expected")
        };
        assert_eq!(latency.selection, selection);
        assert!(latency.millis.is_some());
        let controller_client = Client::new(controller.local());
        assert_eq!(
            controller_client
                .execute(controller_client.prepare(commands[0].clone()))
                .await
                .unwrap(),
            Output::NetworkPresets(Listing::default())
        );
        let Output::NetworkLatency(latency) = controller_client
            .execute(controller_client.prepare(commands[2].clone()))
            .await
            .unwrap()
        else {
            panic!("network latency expected")
        };
        assert!(
            latency.millis.is_none(),
            "controller roots must not replace execution Node roots"
        );
        fail.store(true, Ordering::SeqCst);
        let Output::NetworkPresets(stale) = client
            .execute(client.prepare(commands[1].clone()))
            .await
            .unwrap()
        else {
            panic!("network catalog expected")
        };
        assert!(stale.unavailable);
        assert_eq!(stale.presets, loaded.presets);
        assert_eq!(settings(&client).await, before);
        assert_eq!(node.link().peers().await.unwrap(), node_peers);
        assert_eq!(controller.link().peers().await.unwrap(), controller_peers);
        assert_eq!(node.link().relay_selection().unwrap(), None);
        assert_eq!(controller.link().relay_selection().unwrap(), None);
        assert_eq!(node.id(), NodeId(*node.link().address().id.as_bytes()));
        for request in server.requests.lock().unwrap().iter() {
            assert!(!request.headers.contains_key("authorization"));
            assert!(!request.headers.contains_key("cookie"));
        }
        node.shutdown().await.unwrap();
        controller.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn cancellation_stops_catalog_reads() {
    for remote in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let server =
            http::Server::start(sailry_protocol::conversation::ModelApi::Responses, |_| {
                http::Reply::Hold
            })
            .await;
        let node = Node::start_empty(root.path().join("node")).await.unwrap();
        node.set_network_presets_fixture(&server.endpoint).unwrap();
        let controller = Node::start_empty(root.path().join("controller"))
            .await
            .unwrap();
        let address = controller
            .link()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.link().remote(address)
        } else {
            node.local()
        });
        let pending = tokio::spawn({
            let client = client.clone();
            async move {
                client
                    .execute(client.prepare(Command::ListNetworkPresets { refresh: true }))
                    .await
            }
        });
        server.wait_requests(1).await;
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        server.wait_closed(1).await;
        assert_eq!(
            client
                .execute(client.prepare(Command::ListNetworkPresets { refresh: false }))
                .await
                .unwrap(),
            Output::NetworkPresets(Listing::default())
        );
        node.shutdown().await.unwrap();
        controller.shutdown().await.unwrap();
    }
}
