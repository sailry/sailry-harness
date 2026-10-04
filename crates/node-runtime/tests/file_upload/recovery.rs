use super::*;

#[tokio::test]
async fn restart_preserves_uncertain_results() {
    let fixture = Fixture::start().await;
    let database =
        rusqlite::Connection::open(fixture.profile.join("storage/node.sqlite3")).unwrap();
    database.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let mut requests = Vec::new();
    let mut uploads = Vec::new();
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        let path = format!("file-{index}");
        let upload = prepare(&client, spec(fixture.worktree, &path, b"ours", None)).await;
        stage(&client, &upload, b"ours").await;
        let request = finish(&client, &upload);
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(std::fs::read(fixture.root.join(&path)).unwrap(), b"ours");
        std::fs::write(fixture.root.join(&path), "external").unwrap();
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        requests.push(request);
        uploads.push(upload);
    }
    database.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(database);
    fixture.node.shutdown().await.unwrap();
    let node = Node::start(&fixture.profile).await.unwrap();
    for (index, transport) in [
        node.local(),
        fixture.controller.handle().remote(node.link().address()),
    ]
    .into_iter()
    .enumerate()
    {
        let client = Client::new(transport);
        assert_eq!(
            client
                .execute(requests[index].clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert!(client.open(uploads[index].stream).await.is_err());
        assert_eq!(
            std::fs::read(fixture.root.join(format!("file-{index}"))).unwrap(),
            b"external"
        );
        assert_eq!(
            client
                .execute(finish(&client, &uploads[index]))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let expected = blake3::hash(b"external").to_hex().to_string();
        let upload = prepare(
            &client,
            spec(
                fixture.worktree,
                &format!("file-{index}"),
                b"explicit",
                Some(&expected),
            ),
        )
        .await;
        stage(&client, &upload, b"explicit").await;
        client.execute(finish(&client, &upload)).await.unwrap();
        assert_eq!(
            std::fs::read(fixture.root.join(format!("file-{index}"))).unwrap(),
            b"explicit"
        );
    }
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
