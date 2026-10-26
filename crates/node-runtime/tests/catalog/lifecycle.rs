use super::*;

#[tokio::test]
async fn releases_cancelled_refreshes() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Anthropic, |_| Reply::Hold).await;
        let fixture = Fixture::new(remote, &server).await;
        for count in 1..=2 {
            let client = fixture.client.clone();
            let job = tokio::spawn(async move {
                client
                    .execute(client.prepare(Command::RefreshModelCatalog))
                    .await
            });
            server.wait_requests(count).await;
            assert_eq!(status(&fixture.client).await.revision, 0);
            assert!(matches!(
                execute(&fixture.client, Command::Snapshot).await,
                Output::Snapshot(_)
            ));
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::RefreshModelCatalog))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Busy
            );
            job.abort();
            assert!(job.await.unwrap_err().is_cancelled());
            server.wait_closed(count).await;
        }
        fixture.close().await;
    }
}

#[tokio::test]
async fn shutdown_cancels_downloads() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Anthropic, |_| Reply::Hold).await;
        let fixture = Fixture::new(remote, &server).await;
        let client = fixture.client.clone();
        let job = tokio::spawn(async move {
            client
                .execute(client.prepare(Command::RefreshModelCatalog))
                .await
        });
        server.wait_requests(1).await;
        tokio::time::timeout(Duration::from_secs(5), fixture.node.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), job)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
        server.wait_closed(1).await;
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn failed_replacement_preserves_cache() {
    for remote in [false, true] {
        let count = AtomicUsize::new(0);
        let server = Server::start(ModelApi::Anthropic, move |_| {
            Reply::Json(match count.fetch_add(1, Ordering::SeqCst) {
                1 => json!({"invalid":true}),
                2 => replacement(),
                _ => data(),
            })
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let before = refresh(&fixture.client).await;
        let original = page(&fixture.client, query()).await;
        let events = event_count(&fixture.node).await;
        assert!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::RefreshModelCatalog))
                .await
                .is_err()
        );
        assert_eq!(status(&fixture.client).await, before);
        assert_eq!(page(&fixture.client, query()).await, original);
        fixture.node.with_storage(|db| Box::pin(async move { db.execute_batch("CREATE TRIGGER reject_catalog BEFORE INSERT ON model_catalog BEGIN SELECT RAISE(ABORT,'isolated catalog storage failure'); END;").await.map_err(crate::database_fixture::fault) })).await.unwrap();
        assert!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::RefreshModelCatalog))
                .await
                .is_err()
        );
        assert_eq!(status(&fixture.client).await, before);
        assert_eq!(page(&fixture.client, query()).await, original);
        assert_eq!(event_count(&fixture.node).await, events);
        fixture
            .node
            .with_storage(|db| {
                Box::pin(async move {
                    db.execute_batch("DROP TRIGGER reject_catalog;")
                        .await
                        .map_err(crate::database_fixture::fault)
                })
            })
            .await
            .unwrap();
        assert_eq!(refresh(&fixture.client).await.revision, before.revision + 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn retains_unchanged_rows() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Anthropic, |_| Reply::Json(data())).await;
        let fixture = Fixture::new(remote, &server).await;
        let before = refresh(&fixture.client).await;
        let original = page(&fixture.client, query()).await;
        let events = event_count(&fixture.node).await;
        fixture.node.with_storage(|db| Box::pin(async move {
            db.execute_batch(
                "CREATE TRIGGER reject_catalog_insert BEFORE INSERT ON model_catalog BEGIN SELECT RAISE(ABORT,'unchanged catalog inserted'); END;
                 CREATE TRIGGER reject_catalog_update BEFORE UPDATE ON model_catalog BEGIN SELECT RAISE(ABORT,'unchanged catalog updated'); END;
                 CREATE TRIGGER reject_catalog_delete BEFORE DELETE ON model_catalog BEGIN SELECT RAISE(ABORT,'unchanged catalog deleted'); END;"
            ).await.map_err(crate::database_fixture::fault)
        })).await.unwrap();
        let updated = refresh(&fixture.client).await;
        assert_eq!(updated.revision, before.revision + 1);
        assert_eq!(updated.models, before.models);
        assert_eq!(updated.providers, before.providers);
        assert_eq!(updated.bytes, before.bytes);
        let mut expected = original;
        expected.revision = updated.revision;
        assert_eq!(page(&fixture.client, query()).await, expected);
        assert!(event_count(&fixture.node).await > events);
        fixture.close().await;
    }
}

#[tokio::test]
async fn applies_row_changes() {
    for remote in [false, true] {
        let count = AtomicUsize::new(0);
        let server = Server::start(ModelApi::Anthropic, move |_| {
            Reply::Json(if count.fetch_add(1, Ordering::SeqCst) == 0 {
                data()
            } else {
                replacement()
            })
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let before = refresh(&fixture.client).await;
        fixture.node.with_storage(|db| Box::pin(async move {
            db.execute_batch(
                "CREATE TABLE catalog_changes(operation TEXT,provider TEXT,id TEXT);
                 CREATE TRIGGER catalog_insert AFTER INSERT ON model_catalog BEGIN INSERT INTO catalog_changes VALUES('insert',NEW.provider,NEW.id); END;
                 CREATE TRIGGER catalog_update AFTER UPDATE ON model_catalog BEGIN INSERT INTO catalog_changes VALUES('update',NEW.provider,NEW.id); END;
                 CREATE TRIGGER catalog_delete AFTER DELETE ON model_catalog BEGIN INSERT INTO catalog_changes VALUES('delete',OLD.provider,OLD.id); END;"
            ).await.map_err(crate::database_fixture::fault)
        })).await.unwrap();
        let updated = refresh(&fixture.client).await;
        assert_eq!(updated.revision, before.revision + 1);
        assert_eq!((updated.providers, updated.models), (2, 3));
        let mut all = query();
        all.limit = 100;
        let page = page(&fixture.client, all).await;
        assert_eq!(page.models.len(), 2);
        assert_eq!(page.models[0].id, "added");
        assert_eq!(page.models[1].id, "known");
        assert_eq!(page.models[1].name, "Changed model");
        let (changes, body) = fixture.node.with_storage(|db| Box::pin(async move {
            let mut statement = db.prepare("SELECT operation || ':' || provider || '/' || id FROM catalog_changes ORDER BY operation")
                .await.map_err(crate::database_fixture::fault)?;
            let mut rows = statement.query(()).await.map_err(crate::database_fixture::fault)?;
            let mut changes = Vec::new();
            while let Some(row) = rows.next().await.map_err(crate::database_fixture::fault)? {
                changes.push(row.get::<String>(0).map_err(crate::database_fixture::fault)?);
            }
            let body = db.prepare("SELECT body FROM model_catalog WHERE provider='fixture' AND id='known'")
                .await.map_err(crate::database_fixture::fault)?.query_row(())
                .await.map_err(crate::database_fixture::fault)?.get::<Vec<u8>>(0)
                .map_err(crate::database_fixture::fault)?;
            Ok((changes, body))
        })).await.unwrap();
        assert_eq!(
            changes,
            [
                "delete:fixture/unknown",
                "insert:fixture/added",
                "update:fixture/known"
            ]
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["cost"]["input"],
            2.0
        );
        fixture.close().await;
    }
}

fn replacement() -> Value {
    let mut replacement = data();
    let models = replacement["fixture"]["models"].as_object_mut().unwrap();
    models["a"]["name"] = json!("Changed model");
    models["a"]["cost"]["input"] = json!(2.0);
    models.remove("b");
    models.insert("c".into(), json!({"id":"added","name":"Added model"}));
    replacement
}

async fn event_count(node: &Node) -> i64 {
    node.with_storage(|db| {
        Box::pin(async move {
            db.prepare("SELECT count(*) FROM events")
                .await
                .map_err(crate::database_fixture::fault)?
                .query_row(())
                .await
                .map_err(crate::database_fixture::fault)?
                .get(0)
                .map_err(crate::database_fixture::fault)
        })
    })
    .await
    .unwrap()
}
