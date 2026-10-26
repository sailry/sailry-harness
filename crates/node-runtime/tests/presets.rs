#![cfg(feature = "test-support")]
use crate::discovery_support::{Reply, Server};
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{
    conversation::{ModelApi, discovery, presets::*},
    *,
};
use serde_json::Value;

#[tokio::test]
async fn reads_cached_catalog_without_side_effects() {
    for remote in [false, true] {
        let catalog: Catalog =
            serde_json::from_slice(include_bytes!("fixtures/model-presets.json")).unwrap();
        let value: Value = serde_json::to_value(&catalog).unwrap();
        let server=Server::start(ModelApi::Responses,move |path|if path=="/v1" {Reply::Json(value.clone())} else {Reply::Raw("HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into())}).await;
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        node.set_model_partners_fixture(&server.endpoint).unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let command = Command::ListChannelPresets {
            refresh_partners: false,
        };
        let mut wrong = client.prepare(command.clone());
        wrong.target = NodeId(*controller.handle().address().id.as_bytes());
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        let context = plugin::Context {
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
        };
        for refresh_partners in [false, true] {
            let request = client
                .prepare(Command::ListChannelPresets { refresh_partners })
                .with_plugin(context.clone());
            assert!(client.execute(request).await.is_err());
            assert!(server.requests.lock().unwrap().is_empty());
        }
        let Output::ChannelPresets(initial) = client
            .execute(client.prepare(command.clone()))
            .await
            .unwrap()
        else {
            panic!("channel catalog expected")
        };
        assert_eq!(initial.entries.len(), 33);
        assert!(!initial.partners_unavailable);
        assert!(server.requests.lock().unwrap().is_empty());
        let request = client.prepare(Command::ListChannelPresets {
            refresh_partners: true,
        });
        assert!(!request.command.durable());
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert_eq!(admission.receipt.id, request.id);
        assert!(!admission.receipt.durable);
        let Output::ChannelPresets(loaded) = admission.completion.await.unwrap().unwrap() else {
            panic!("channel catalog expected")
        };
        let partner = loaded
            .entries
            .iter()
            .find(|entry| entry.source == Source::Partner)
            .unwrap();
        assert_eq!(partner.preset, catalog.partners[0]);
        assert!(!loaded.partners_unavailable);
        assert_eq!(
            client
                .execute(client.prepare(command.clone()))
                .await
                .unwrap(),
            Output::ChannelPresets(loaded.clone())
        );
        let Output::DiscoveredModels(models) =
            client
                .execute(client.prepare(Command::DiscoverModels(Box::new(
                    discovery::Source::Draft(discovery::Draft {
                        provider: ProviderId::new(),
                        api: ModelApi::Responses,
                        endpoint: "https://example.com/v1".into(),
                        credential: None,
                        secret: None,
                        preset: Some(PresetRef {
                            source: Source::Partner,
                            id: "example".into(),
                            route: None,
                        }),
                    }),
                ))))
                .await
                .unwrap()
        else {
            panic!("static model identifiers expected")
        };
        assert_eq!(
            models
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["example-text", "example-vision"]
        );
        assert!(
            models
                .models
                .iter()
                .all(|model| model.capabilities.is_none())
        );
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].method, "GET");
            assert!(!requests[0].headers.contains_key("authorization"));
            assert!(!requests[0].headers.contains_key("cookie"));
        }
        node.set_model_partners_fixture(&format!("{}/unavailable", server.endpoint))
            .unwrap();
        let Output::ChannelPresets(fallback) = client
            .execute(client.prepare(Command::ListChannelPresets {
                refresh_partners: true,
            }))
            .await
            .unwrap()
        else {
            panic!("channel catalog expected")
        };
        assert!(fallback.partners_unavailable);
        assert_eq!(fallback.entries, loaded.entries);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start_empty(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::ChannelPresets(resumed) =
            client.execute(client.prepare(command)).await.unwrap()
        else {
            panic!("channel catalog expected")
        };
        assert_eq!(resumed.entries, loaded.entries);
        assert!(!resumed.partners_unavailable);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let (database, db) =
            crate::database_fixture::open(&profile.join("storage/node.turso"), true).await;
        assert_eq!(
            db.prepare("SELECT count(*) FROM requests")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<i64>(0)
                .unwrap(),
            0
        );
        assert_eq!(
            db.prepare("SELECT count(*) FROM providers")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<i64>(0)
                .unwrap(),
            0
        );
        crate::database_fixture::close(database, db).await;
    }
}
