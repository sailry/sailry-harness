use super::*;

#[tokio::test]
async fn preserves_uncertain_publication() {
    let fixture = Fixture::start().await;
    std::fs::write(fixture.source.root.join("file"), "source contents").unwrap();
    let database =
        rusqlite::Connection::open(fixture.destination.profile.join("storage/node.sqlite3"))
            .unwrap();
    database.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut pending = Vec::new();
    for pair in PATHS {
        let (source, target) = fixture.clients(pair);
        let download = prepare(&source, fixture.source.worktree, "file").await;
        let path = format!("{}-{}", pair.0, pair.1);
        let request = target
            .stage_copy(
                &source,
                &download,
                spec(&download, fixture.destination.worktree, &path),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        assert_eq!(
            target.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read(fixture.destination.root.join(&path)).unwrap(),
            b"source contents"
        );
        std::fs::write(fixture.destination.root.join(&path), "external").unwrap();
        assert_eq!(
            target.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        pending.push((pair.1, path, request));
    }
    database.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(database);
    fixture.destination.node.shutdown().await.unwrap();
    // Source availability is not required to recover an admitted destination request.
    fixture.source.node.shutdown().await.unwrap();
    let node = Node::start(&fixture.destination.profile).await.unwrap();
    let transports = [
        node.local(),
        fixture.controller.handle().remote(node.link().address()),
    ];
    for (index, path, request) in pending {
        let client = Client::new(transports[index].clone());
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read(fixture.destination.root.join(path)).unwrap(),
            b"external"
        );
        assert_eq!(
            client
                .execute(client.prepare(request.command))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    assert_eq!(
        std::fs::read(fixture.source.root.join("file")).unwrap(),
        b"source contents"
    );
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
