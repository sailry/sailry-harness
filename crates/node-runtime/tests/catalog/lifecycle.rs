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
            if count.fetch_add(1, Ordering::SeqCst) == 1 {
                Reply::Json(json!({"invalid":true}))
            } else {
                Reply::Json(data())
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let before = refresh(&fixture.client).await;
        let original = page(&fixture.client, query()).await;
        let db = rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
            .unwrap();
        let events: i64 = db
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::RefreshModelCatalog))
                .await
                .is_err()
        );
        assert_eq!(status(&fixture.client).await, before);
        assert_eq!(page(&fixture.client, query()).await, original);
        db.execute_batch("CREATE TRIGGER reject_catalog BEFORE INSERT ON model_catalog BEGIN SELECT RAISE(ABORT,'isolated catalog storage failure'); END;").unwrap();
        assert!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::RefreshModelCatalog))
                .await
                .is_err()
        );
        assert_eq!(status(&fixture.client).await, before);
        assert_eq!(page(&fixture.client, query()).await, original);
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM events", [], |row| row.get(0))
                .unwrap(),
            events
        );
        db.execute_batch("DROP TRIGGER reject_catalog;").unwrap();
        assert_eq!(refresh(&fixture.client).await.revision, before.revision + 1);
        fixture.close().await;
    }
}
