use super::*;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn releases_uploads_on_shutdown() {
    let fixture = Fixture::start().await;
    let mut uploads = Vec::new();
    let mut streams = Vec::new();
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        let mut owned = Vec::new();
        for phase in ["pending", "active", "staged"] {
            let upload = prepare(
                &client,
                spec(
                    fixture.worktree,
                    &format!("{index}-{phase}"),
                    b"content",
                    None,
                ),
            )
            .await;
            if phase == "active" {
                streams.push(client.open(upload.stream).await.unwrap());
            }
            if phase == "staged" {
                stage(&client, &upload, b"content").await;
            }
            owned.push(upload);
        }
        uploads.push(owned);
    }
    fixture.node.shutdown().await.unwrap();
    for mut stream in streams {
        let result = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut [0]))
            .await
            .unwrap();
        assert!(result.is_err() || result.unwrap() == 0);
    }
    assert_eq!(std::fs::read_dir(&fixture.root).unwrap().count(), 0);
    let node = Node::start(&fixture.profile).await.unwrap();
    for (index, transport) in [
        node.local(),
        fixture.controller.handle().remote(node.link().address()),
    ]
    .into_iter()
    .enumerate()
    {
        let client = Client::new(transport);
        for upload in &uploads[index] {
            assert!(client.open(upload.stream).await.is_err());
            assert_eq!(
                client
                    .execute(finish(&client, upload))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
            let fresh = prepare(&client, upload.spec.clone()).await;
            stage(&client, &fresh, b"content").await;
            client.execute(finish(&client, &fresh)).await.unwrap();
            assert_eq!(
                std::fs::read(fixture.root.join(&upload.spec.path)).unwrap(),
                b"content"
            );
        }
    }
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalidates_live_subscriptions() {
    let fixture = Fixture::start().await;
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        let mut watch = transport
            .subscribe(Topic::Files(fixture.worktree))
            .await
            .unwrap();
        watch.next().await.unwrap();
        let upload = prepare(
            &client,
            spec(fixture.worktree, &format!("file-{index}"), b"content", None),
        )
        .await;
        stage(&client, &upload, b"content").await;
        client.execute(finish(&client, &upload)).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), watch.next())
                .await
                .unwrap()
                .unwrap(),
            Update::FilesChanged {
                node: fixture.node.id(),
                worktree: fixture.worktree
            }
        );
    }
    fixture.close().await;
}
