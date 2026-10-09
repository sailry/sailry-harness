use super::*;
use sailry_protocol::models::{Hub, Kind, Search as ModelSearch, Source as ModelSource};
use tokio::net::TcpListener;

#[tokio::test]
async fn captures_saved_mirror_for_each_controller() {
    let fixture = Fixture::start().await;
    for client in fixture.clients() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let Output::KnowledgeSettings(mut settings) =
            execute(&client, Command::ReadKnowledgeSettings).await
        else {
            panic!("knowledge settings expected");
        };
        settings.mirror = Some(format!("https://{}/cache/", listener.local_addr().unwrap()));
        let Output::KnowledgeSettings(saved) =
            execute(&client, Command::SaveKnowledgeSettings(settings.clone())).await
        else {
            panic!("saved knowledge settings expected");
        };
        assert_eq!(saved.mirror, settings.mirror);
        for command in [
            Command::SearchModels(ModelSearch {
                hub: Hub::HuggingFace,
                kind: Kind::Embedding,
                query: "fixture".into(),
                cursor: None,
            }),
            Command::ReadModelVersions {
                hub: Hub::HuggingFace,
                repository: "owner/fixture".into(),
            },
            Command::ResolveModel {
                kind: Kind::Embedding,
                source: ModelSource {
                    hub: Hub::HuggingFace,
                    repository: "owner/fixture".into(),
                    revision: "a".repeat(40),
                },
            },
        ] {
            // A local TLS connection proves endpoint selection without custom trust or external network access.
            let (result, accepted) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(client.execute(client.prepare(command)), async {
                    let (socket, _) = listener.accept().await.unwrap();
                    drop(socket);
                    true
                })
            })
            .await
            .expect("catalog used the saved mirror");
            assert!(accepted);
            assert_eq!(result.unwrap_err().code, ErrorCode::Unavailable);
        }
    }
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
}
