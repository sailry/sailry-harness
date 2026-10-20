use super::*;
use sailry_protocol::models::{Hub, Kind, Search as ModelSearch, Source as ModelSource};
use tokio::net::TcpListener;

#[cfg(feature = "test-support")]
mod fixture;

#[cfg(feature = "test-support")]
#[tokio::test]
async fn refreshes_the_registered_ocr_profile() {
    let fixture = Fixture::start().await;
    let catalog = self::fixture::Catalog::start().await;
    for hub in [Hub::HuggingFace, Hub::ModelScope] {
        fixture
            .node
            .set_model_catalog_fixture(hub, &catalog.endpoint)
            .unwrap();
        for client in fixture.clients() {
            let Output::ModelDefinition(mut previous) = execute(
                &client,
                Command::ResolveModel {
                    kind: Kind::Ocr,
                    source: ModelSource {
                        hub,
                        repository: "org/model".into(),
                        revision: "HEAD".into(),
                    },
                },
            )
            .await
            else {
                panic!("OCR definition expected")
            };
            previous.name = "Chosen mobile profile".into();
            previous.source.revision = "b".repeat(40);
            for asset in &mut previous.assets {
                asset.source.revision = "b".repeat(40);
            }
            previous
                .assets
                .sort_by(|left, right| left.name.cmp(&right.name));
            let Output::LocalModel { id } = execute(
                &client,
                Command::RegisterLocalModel {
                    definition: previous.clone(),
                },
            )
            .await
            else {
                panic!("OCR ID expected")
            };
            let admission = client
                .dispatch(client.prepare(Command::RefreshLocalModel { id }))
                .await
                .unwrap();
            assert!(!admission.receipt.durable);
            let Output::ModelDefinition(latest) = admission.completion.await.unwrap().unwrap()
            else {
                panic!("updated OCR definition expected")
            };
            assert_eq!(latest.name, previous.name);
            assert_eq!(latest.format, previous.format);
            assert_eq!(
                latest.format,
                sailry_protocol::models::Format::PaddleOcr {
                    family: sailry_protocol::models::PaddleFamily::V5,
                    orientation: false
                }
            );
            assert_eq!(
                latest
                    .assets
                    .iter()
                    .map(|asset| (&asset.name, &asset.path))
                    .collect::<Vec<_>>(),
                previous
                    .assets
                    .iter()
                    .map(|asset| (&asset.name, &asset.path))
                    .collect::<Vec<_>>()
            );
            assert!(
                latest
                    .assets
                    .iter()
                    .all(|asset| asset.source.revision == self::fixture::REVISION)
            );
            assert!(
                latest
                    .assets
                    .iter()
                    .any(|asset| asset.path == "pp-ocrv5_mobile_rec.onnx")
            );
        }
    }
    fixture.close().await;
}

#[tokio::test]
async fn captures_saved_mirror_for_each_controller() {
    let fixture = Fixture::start().await;
    for client in fixture.clients() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let Output::LocalModelSettings(mut settings) =
            execute(&client, Command::ReadLocalModelSettings).await
        else {
            panic!("knowledge settings expected");
        };
        settings.mirror = Some(format!("https://{}/cache/", listener.local_addr().unwrap()));
        let Output::LocalModelSettings(saved) =
            execute(&client, Command::SaveLocalModelSettings(settings.clone())).await
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
            let Output::LocalModel { id: model } = execute(
                &client,
                Command::RegisterLocalModel {
                    definition: selected.clone(),
                },
            )
            .await
            else {
                panic!("registered model expected");
            };
            let Output::LocalModels(models) = execute(
                &client,
                Command::ReadLocalModels {
                    kind: Some(sailry_protocol::models::Kind::Embedding),
                },
            )
            .await
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
                    .find(|info| info.id == model)
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
