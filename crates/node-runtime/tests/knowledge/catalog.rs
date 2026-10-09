use super::*;
use sailry_protocol::models::{Hub, Kind, Search as ModelSearch, Source as ModelSource};
use tokio::net::TcpListener;

#[cfg(feature = "test-support")]
mod fixture;

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

#[cfg(feature = "test-support")]
#[tokio::test]
async fn shares_compatible_pages_and_immutable_resolution() {
    let fixture = Fixture::start().await;
    let catalog = self::fixture::Catalog::start().await;
    for hub in [Hub::HuggingFace, Hub::ModelScope] {
        fixture
            .node
            .set_model_catalog_fixture(hub, &catalog.endpoint)
            .unwrap();
        for client in fixture.clients() {
            catalog.clear();
            let request = client.prepare(Command::SearchModels(ModelSearch {
                hub,
                kind: Kind::Embedding,
                query: String::new(),
                cursor: None,
            }));
            let admission = client.dispatch(request).await.unwrap();
            assert!(!admission.receipt.durable);
            let Output::ModelSearch(first) = admission.completion.await.unwrap().unwrap() else {
                panic!("model page expected");
            };
            assert_eq!(first.items.len(), 1);
            assert_eq!(first.items[0].source.revision, self::fixture::REVISION);
            assert_eq!(
                first.items[0].format,
                sailry_protocol::models::Format::Transformer {
                    dimension: 8,
                    pooling: sailry_protocol::models::Pooling::Cls,
                    token_types: true
                }
            );
            assert!(!first.partial);
            let Output::ModelSearch(second) = execute(
                &client,
                Command::SearchModels(ModelSearch {
                    hub,
                    kind: Kind::Embedding,
                    query: String::new(),
                    cursor: first.next_cursor,
                }),
            )
            .await
            else {
                panic!("model page expected");
            };
            assert_eq!(second.items.len(), 1);
            assert_eq!(second.items[0].source.revision, self::fixture::NEWER);
            assert!(second.next_cursor.is_none());
            assert!(!second.partial);
            let reads = catalog.requests();
            assert!(
                reads
                    .iter()
                    .all(|read| !read.path.ends_with("tokenizer.json")
                        && !read.path.ends_with(".onnx")
                        && !matches!(
                            read.query.get("FilePath").map(String::as_str),
                            Some("tokenizer.json" | "onnx/model.onnx")
                        ))
            );
            let Output::ModelDefinition(head) = execute(
                &client,
                Command::ResolveModel {
                    kind: Kind::Embedding,
                    source: ModelSource {
                        revision: "HEAD".into(),
                        ..first.items[0].source.clone()
                    },
                },
            )
            .await
            else {
                panic!("model definition expected");
            };
            assert_eq!(head.source, first.items[0].source);
            head.validate().unwrap();
            assert!(
                head.assets
                    .iter()
                    .all(|asset| asset.source.revision == self::fixture::REVISION)
            );
            let Output::ModelDefinition(selected) = execute(
                &client,
                Command::ResolveModel {
                    kind: Kind::Embedding,
                    source: second.items[0].source.clone(),
                },
            )
            .await
            else {
                panic!("model definition expected");
            };
            assert_eq!(selected.source, second.items[0].source);
            assert!(
                selected
                    .assets
                    .iter()
                    .all(|asset| asset.source.revision == self::fixture::NEWER)
            );
            let Output::KnowledgeModel { model } = execute(
                &client,
                Command::RegisterKnowledgeModel {
                    definition: selected.clone(),
                },
            )
            .await
            else {
                panic!("registered model expected");
            };
            let Output::KnowledgeModels(models) =
                execute(&client, Command::ReadKnowledgeModels).await
            else {
                panic!("model registry expected");
            };
            let mut expected = selected;
            expected
                .assets
                .sort_by(|left, right| left.name.cmp(&right.name));
            assert_eq!(
                models
                    .iter()
                    .find(|info| info.model == model)
                    .unwrap()
                    .definition,
                expected
            );
            // Full resolution is intentionally the first operation to read tokenizer bytes.
            let reads = catalog.requests();
            assert!(
                reads
                    .iter()
                    .any(|read| read.path.ends_with("tokenizer.json")
                        || read.query.get("FilePath").map(String::as_str)
                            == Some("tokenizer.json"))
            );
            assert!(reads.iter().all(|read| !read.path.ends_with(".onnx")
                && read.query.get("FilePath").map(String::as_str) != Some("onnx/model.onnx")));
        }
    }
    fixture.close().await;
}
