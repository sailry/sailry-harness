use super::*;
use turso::{Connection, params};

async fn records(db: &Connection, model: &str, collection: KnowledgeId) -> [Vec<u8>; 3] {
    let row = db
        .prepare(
            "SELECT (SELECT body FROM local_models WHERE id=?1),
                    (SELECT body FROM local_model_settings WHERE singleton=1),
                    (SELECT body FROM knowledge_collections WHERE id=?2)",
        )
        .await
        .unwrap()
        .query_row(params![model, collection.to_string()])
        .await
        .unwrap();
    [
        row.get(0).unwrap(),
        row.get(1).unwrap(),
        row.get(2).unwrap(),
    ]
}

#[tokio::test]
async fn preserves_registered_models_through_both_links() {
    for remote in [false, true] {
        let fixture = Fixture::start().await;
        fs::create_dir(fixture.root.join("registered")).unwrap();
        fs::write(fixture.root.join("registered/alpha.txt"), "alpha").unwrap();
        fs::write(fixture.root.join("registered/beta.txt"), "beta").unwrap();
        let client = fixture
            .clients()
            .into_iter()
            .nth(usize::from(remote))
            .unwrap();
        let assets = fixture::Assets::new(2, &[100., 0., 1., 0., 0., 1.]);
        let model = register(&client, &assets.definition).await;
        let profile = fixture.temp.path().join("node");
        let path = assets.seed(&profile, &model);
        assert_eq!(
            execute(&client, Command::InstallLocalModel { id: model.clone() }).await,
            Output::LocalModel { id: model.clone() }
        );
        let receipt = fs::read(path.join("validated")).unwrap();
        assert_eq!(receipt.len(), 64);
        let mut definition = definition(Source::Worktree {
            worktree: fixture.worktree,
            path: "registered".into(),
        });
        definition.model = Some(model.clone());
        let collection = saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition,
                    plugin: None,
                },
            )
            .await,
        );
        let collection = ready(&client, collection.id, Some(model.clone())).await;
        semantic(&client, collection.id, model.clone()).await;
        let mut input = query("alpha", collection.id);
        input.mode = Mode::Semantic;
        let citation = search(&client, None, input.clone()).await.hits[0]
            .citation
            .clone();
        let citation_request = CitationRequest {
            collection: collection.id,
            chunk: citation.chunk.clone(),
            revision: citation.revision.clone(),
            context: false,
            offset: 0,
            limit: 4096,
        };
        let original = execute(
            &client,
            Command::ReadKnowledgeCitation(citation_request.clone()),
        )
        .await;
        drop(client);
        let Fixture {
            temp,
            root,
            node,
            controller,
            worktree: _,
        } = fixture;
        let node_id = node.id();
        node.shutdown().await.unwrap();
        let (handle, mut db) =
            crate::knowledge::database_fixture::open(&profile.join("storage/node.turso"), false)
                .await;
        let transaction = db.transaction().await.unwrap();
        // Current v1 fills omitted choices without importing retired model formats.
        let settings = serde_json::to_vec(&serde_json::json!({
            "revision": 0,
            "mirror": null,
            "choices": {"embedding": {"enabled": true, "model": model.as_str()}}
        }))
        .unwrap();
        assert_eq!(
            transaction
                .execute(
                    "UPDATE local_model_settings SET body=?1 WHERE singleton=1",
                    [settings],
                )
                .await
                .unwrap(),
            1
        );
        transaction
            .execute_batch("DROP INDEX events_kind")
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let stored = records(&db, &model, collection.id).await;
        crate::knowledge::database_fixture::close(handle, db).await;
        let node = Node::start_empty_with_plugin_catalog(&profile, "http://127.0.0.1:9/")
            .await
            .unwrap();
        assert_eq!(node.id(), node_id);
        let client = if remote {
            Client::new(controller.handle().remote(node.link().address()))
        } else {
            Client::new(node.local())
        };
        let Output::LocalModelSettings(settings) =
            execute(&client, Command::ReadLocalModelSettings).await
        else {
            panic!("local settings expected")
        };
        assert_eq!(settings.revision, 0);
        assert_eq!(
            settings.selected(sailry_protocol::models::Kind::Embedding),
            Some(model.as_str())
        );
        for kind in [
            sailry_protocol::models::Kind::Ocr,
            sailry_protocol::models::Kind::Decision,
        ] {
            assert_eq!(
                settings.choices.get(&kind),
                Some(&sailry_protocol::models::Choice {
                    enabled: true,
                    model: None,
                })
            );
        }
        let registered = models(&client)
            .await
            .into_iter()
            .find(|record| record.id == model)
            .unwrap();
        assert_eq!(registered.state, ModelState::Installed);
        let restored = list(&client)
            .await
            .into_iter()
            .find(|entry| entry.id == collection.id)
            .unwrap();
        assert_eq!(restored.revision, collection.revision);
        assert_eq!(restored.rebuild, collection.rebuild);
        assert_eq!(restored.progress.generation, collection.progress.generation);
        assert_eq!(restored.definition, collection.definition);
        assert_eq!(restored.definition.model.as_deref(), Some(model.as_str()));
        ready(&client, collection.id, Some(model.clone())).await;
        semantic(&client, collection.id, model.clone()).await;
        assert_eq!(
            search(&client, None, input).await.hits[0].citation,
            citation
        );
        assert_eq!(
            execute(&client, Command::ReadKnowledgeCitation(citation_request)).await,
            original
        );
        drop(client);
        node.shutdown().await.unwrap();
        let (handle, db) =
            crate::knowledge::database_fixture::open(&profile.join("storage/node.turso"), true)
                .await;
        let current = records(&db, &model, collection.id).await;
        assert_eq!(&current[..2], &stored[..2]);
        let current_capture: serde_json::Value = serde_json::from_slice(&current[2]).unwrap();
        let stored_capture: serde_json::Value = serde_json::from_slice(&stored[2]).unwrap();
        assert_eq!(
            current_capture.get("capture").unwrap(),
            stored_capture.get("capture").unwrap()
        );
        assert_eq!(
            db.prepare("SELECT COUNT(*) FROM sqlite_schema WHERE name='events_kind'")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<i64>(0)
                .unwrap(),
            1
        );
        crate::knowledge::database_fixture::close(handle, db).await;
        let backups = fs::read_dir(profile.join("storage/backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path().join("node.turso"))
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        let (handle, db) = crate::knowledge::database_fixture::open(&backups[0], true).await;
        assert_eq!(records(&db, &model, collection.id).await, stored);
        assert_eq!(
            db.prepare("SELECT COUNT(*) FROM sqlite_schema WHERE name='events_kind'")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get::<i64>(0)
                .unwrap(),
            0
        );
        crate::knowledge::database_fixture::close(handle, db).await;
        assert_eq!(fs::read(path.join("validated")).unwrap(), receipt);
        assert_eq!(
            fs::read(root.join("registered/alpha.txt")).unwrap(),
            b"alpha"
        );
        assert_eq!(fs::read(root.join("registered/beta.txt")).unwrap(), b"beta");
        controller.close().await.unwrap();
        drop(temp);
    }
}
