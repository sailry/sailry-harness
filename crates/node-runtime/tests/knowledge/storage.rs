use super::*;
use serde_json::json;

#[tokio::test]
async fn reads_raw_values_and_explicitly_shared_citations() {
    let fixture = Fixture::start().await;
    for (index, client) in fixture.clients().into_iter().enumerate() {
        let name = format!("corpus-{index}");
        let owner = install(
            &client,
            &fixture.root,
            fixture.worktree,
            &name,
            0,
            &[
                Action::ReadStorage,
                Action::WriteStorage,
                Action::ReadKnowledge,
                Action::WriteKnowledge,
            ],
        )
        .await;
        let reader = install(
            &client,
            &fixture.root,
            fixture.worktree,
            &format!("reader-{index}"),
            0,
            &[Action::ReadKnowledge, Action::WriteKnowledge],
        )
        .await;
        let outsider = install(
            &client,
            &fixture.root,
            fixture.worktree,
            &format!("outsider-{index}"),
            0,
            &[
                Action::ReadStorage,
                Action::WriteStorage,
                Action::ReadKnowledge,
            ],
        )
        .await;
        let raw = json!({"text":"我们使用向量数据库。This is the original source."});
        run(
            &client,
            &owner,
            Command::WritePluginValue {
                key: "body/raw".into(),
                value: raw.clone(),
                expected_revision: 0,
            },
        )
        .await;
        run(
            &client,
            &owner,
            Command::WriteIndexedPluginValue {
                key: "notes/one".into(),
                value: json!({"title":"Source title"}),
                expected_revision: 0,
                index: plugin::storage::Index {
                    fields: ["Precut title".into(), "PRETOKENIZED_ONLY".into()],
                    tags: vec!["active".into()],
                    order: 1,
                    source: Some(plugin::storage::Source {
                        key: "body/raw".into(),
                        pointer: "/text".into(),
                    }),
                },
            },
        )
        .await;
        run(
            &client,
            &owner,
            Command::WritePluginValue {
                key: "notes/json".into(),
                value: json!({"body":"未声明索引的原始数据"}),
                expected_revision: 0,
            },
        )
        .await;
        run(
            &client,
            &outsider,
            Command::WritePluginValue {
                key: "body/raw".into(),
                value: json!({"text":"Private other namespace"}),
                expected_revision: 0,
            },
        )
        .await;
        let definition = definition(Source::Storage {
            scope: plugin::storage::Scope::Node,
            prefix: "notes/".into(),
        });
        let collection = super::saved(
            run(
                &client,
                &owner,
                Command::CreateKnowledgeCollection {
                    definition,
                    plugin: None,
                },
            )
            .await,
        );
        assert_eq!(collection.owner, Owner::Plugin { name: name.clone() });
        let page = found(&client, Some(&owner), "量数", collection.id).await;
        assert_eq!(page.hits[0].title, "Source title");
        assert!(page.hits[0].text.contains("我们使用向量数据库"));
        assert_eq!(
            page.hits[0].citation.position,
            Position::Value {
                key: "body/raw".into(),
                pointer: "/text".into()
            }
        );
        let citation = &page.hits[0].citation;
        let opened = run(
            &client,
            &owner,
            Command::ReadKnowledgeCitation(CitationRequest {
                context: false,
                collection: citation.collection,
                chunk: citation.chunk.clone(),
                revision: citation.revision.clone(),
                offset: 0,
                limit: 4096,
            }),
        )
        .await;
        let Output::KnowledgeCitation(opened) = opened else {
            panic!("citation expected")
        };
        assert!(opened.text.contains("我们使用向量数据库"));
        assert_eq!(opened.citation.position, citation.position);
        let mut tagged = query("向量", collection.id);
        tagged.tags = vec!["active".into()];
        assert_eq!(search(&client, Some(&owner), tagged).await.hits.len(), 1);
        assert!(
            search(
                &client,
                Some(&owner),
                query("PRETOKENIZED_ONLY", collection.id)
            )
            .await
            .hits
            .is_empty()
        );
        let generic = found(&client, Some(&owner), "原始", collection.id).await;
        assert_eq!(
            generic.hits[0].citation.position,
            Position::Value {
                key: "notes/json".into(),
                pointer: "".into()
            }
        );
        assert!(generic.hits[0].text.contains("未声明索引的原始数据"));
        assert!(
            search(&client, Some(&reader), query("向量", collection.id))
                .await
                .hits
                .is_empty()
        );
        assert!(
            search(&client, Some(&outsider), query("向量", collection.id))
                .await
                .hits
                .is_empty()
        );
        let mut shared = collection.definition.clone();
        shared.shared_with = vec![reader.package.name.clone()];
        let updated = super::saved(
            execute(
                &client,
                Command::UpdateKnowledgeCollection {
                    id: collection.id,
                    expected_revision: 1,
                    definition: shared,
                },
            )
            .await,
        );
        let received = found(&client, Some(&reader), "向量", collection.id).await;
        assert_eq!(received.hits[0].citation.collection, collection.id);
        assert!(
            search(&client, Some(&outsider), query("向量", collection.id))
                .await
                .hits
                .is_empty()
        );
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::DeleteKnowledgeCollection {
                            id: collection.id,
                            expected_revision: updated.revision
                        })
                        .with_plugin(reader.clone())
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        run(
            &client,
            &owner,
            Command::WritePluginValue {
                key: "body/raw".into(),
                value: json!({"text":"原始正文已改为海量新记录"}),
                expected_revision: 1,
            },
        )
        .await;
        let changed = found(&client, Some(&owner), "新记录", collection.id).await;
        assert_ne!(
            page.hits[0].citation.revision,
            changed.hits[0].citation.revision
        );
        let stale = client
            .execute(
                client
                    .prepare(Command::ReadKnowledgeCitation(CitationRequest {
                        context: false,
                        collection: citation.collection,
                        chunk: citation.chunk.clone(),
                        revision: citation.revision.clone(),
                        offset: 0,
                        limit: 4096,
                    }))
                    .with_plugin(owner.clone()),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            stale.code,
            ErrorCode::RevisionConflict | ErrorCode::NotFound
        ));
        assert!(
            search(&client, Some(&owner), query("向量", collection.id))
                .await
                .hits
                .is_empty()
        );
        let fresh = install(
            &client,
            &fixture.root,
            fixture.worktree,
            &name,
            1,
            &[
                Action::ReadStorage,
                Action::WriteStorage,
                Action::ReadKnowledge,
            ],
        )
        .await;
        assert_ne!(fresh.package.digest, owner.package.digest);
        assert_eq!(
            client
                .execute(
                    client
                        .prepare(Command::ListKnowledgeCollections)
                        .with_plugin(owner)
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn rebuilds_deleted_derived_index_after_restart() {
    for remote in [false, true] {
        let fixture = Fixture::start().await;
        let client = fixture
            .clients()
            .into_iter()
            .nth(usize::from(remote))
            .unwrap();
        let owner = install(
            &client,
            &fixture.root,
            fixture.worktree,
            "restored-corpus",
            0,
            &[
                Action::ReadStorage,
                Action::WriteStorage,
                Action::ReadKnowledge,
                Action::WriteKnowledge,
            ],
        )
        .await;
        let original = json!("原始知识数据保留完整");
        run(
            &client,
            &owner,
            Command::WritePluginValue {
                key: "raw".into(),
                value: original.clone(),
                expected_revision: 0,
            },
        )
        .await;
        let collection = super::saved(
            run(
                &client,
                &owner,
                Command::CreateKnowledgeCollection {
                    definition: definition(Source::Storage {
                        scope: plugin::storage::Scope::Node,
                        prefix: "".into(),
                    }),
                    plugin: None,
                },
            )
            .await,
        );
        found(&client, Some(&owner), "知识", collection.id).await;
        let previous = run(
            &client,
            &owner,
            Command::ReadPluginValue { key: "raw".into() },
        )
        .await;
        let Fixture {
            temp,
            root: _,
            node,
            controller,
            worktree: _,
        } = fixture;
        drop(client);
        node.shutdown().await.unwrap();
        let profile = temp.path().join("node");
        fs::remove_file(profile.join("storage/knowledge.db")).unwrap();
        let node = Node::start_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let restored = list(&client)
            .await
            .into_iter()
            .find(|item| item.id == collection.id)
            .unwrap();
        assert_eq!(restored.revision, collection.revision);
        assert_eq!(restored.progress.generation, collection.progress.generation);
        assert_eq!(restored.definition, collection.definition);
        let page = found(&client, Some(&owner), "保留", collection.id).await;
        assert!(page.hits[0].text.contains("原始知识数据保留完整"));
        assert_eq!(
            run(
                &client,
                &owner,
                Command::ReadPluginValue { key: "raw".into() }
            )
            .await,
            previous
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
