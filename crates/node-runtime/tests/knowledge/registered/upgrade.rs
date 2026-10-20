use super::*;
use rusqlite::params;

#[tokio::test]
async fn imports_installed_models_through_both_links() {
    for remote in [false, true] {
        let fixture = Fixture::start().await;
        fs::create_dir(fixture.root.join("imported")).unwrap();
        fs::write(fixture.root.join("imported/alpha.txt"), "alpha").unwrap();
        fs::write(fixture.root.join("imported/beta.txt"), "beta").unwrap();
        let client = fixture.clients().into_iter().next().unwrap();
        let collection = saved(
            execute(
                &client,
                Command::CreateKnowledgeCollection {
                    definition: definition(Source::Worktree {
                        worktree: fixture.worktree,
                        path: "imported".into(),
                    }),
                    plugin: None,
                },
            )
            .await,
        );
        drop(client);
        let Fixture {
            temp,
            root: _,
            node,
            controller,
            worktree: _,
        } = fixture;
        let node_id = node.id();
        node.shutdown().await.unwrap();
        let profile = temp.path().join("node");
        let assets = fixture::Assets::new(2, &[100., 0., 1., 0., 0., 1.]);
        let id = ModelId::new();
        let model = format!("model-{id}");
        let path = assets.seed(&profile, &model);
        let body = serde_json::to_vec(&assets.definition).unwrap();
        fs::write(
            path.join("validated"),
            blake3::hash(&body).to_hex().as_bytes(),
        )
        .unwrap();
        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        db.execute_batch("DROP TABLE local_models; DROP TABLE local_model_settings;
            CREATE TABLE knowledge_models(id TEXT PRIMARY KEY,digest TEXT NOT NULL UNIQUE,body BLOB NOT NULL);
            CREATE TABLE knowledge_settings(singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL);
            CREATE TABLE ocr_settings(singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL);").unwrap();
        db.execute(
            "INSERT INTO knowledge_models VALUES(?1,?2,?3)",
            params![
                id.to_string(),
                blake3::hash(&body).to_hex().to_string(),
                body
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO knowledge_settings VALUES(1,?1)",
            [serde_json::to_vec(
                &serde_json::json!({"revision":23,"model":{"registered":{"id":id}},"mirror":null}),
            )
            .unwrap()],
        )
        .unwrap();
        let stored: Vec<u8> = db
            .query_row(
                "SELECT body FROM knowledge_collections WHERE id=?1",
                [collection.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let mut stored: serde_json::Value = serde_json::from_slice(&stored).unwrap();
        stored["collection"]["definition"]["model"] = serde_json::json!({"registered":{"id":id}});
        stored["collection"]["progress"]["model"] = serde_json::json!({"registered":{"id":id}});
        db.execute(
            "UPDATE knowledge_collections SET body=?1 WHERE id=?2",
            params![
                serde_json::to_vec(&stored).unwrap(),
                collection.id.to_string()
            ],
        )
        .unwrap();
        let generation = stored["collection"]["progress"]["generation"]
            .as_u64()
            .unwrap();
        db.close().unwrap();
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
        let imported = models(&client)
            .await
            .into_iter()
            .find(|record| record.id == model)
            .unwrap();
        assert_eq!(imported.state, ModelState::Installed);
        let restored = list(&client)
            .await
            .into_iter()
            .find(|entry| entry.id == collection.id)
            .unwrap();
        assert_eq!(restored.revision, collection.revision);
        assert_eq!(restored.rebuild, collection.rebuild);
        assert_eq!(restored.progress.generation, generation);
        assert_eq!(restored.definition.model.as_deref(), Some(model.as_str()));
        ready(&client, collection.id, Some(model.clone())).await;
        semantic(&client, collection.id, model).await;
        assert_eq!(
            fs::read_dir(profile.join("storage/backups"))
                .unwrap()
                .count(),
            1
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
