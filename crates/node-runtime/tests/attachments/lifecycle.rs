use super::*;

#[tokio::test]
async fn restores_receipts() {
    let mut fixture = Fixture::start().await;
    let profile = fixture.node.profile().to_owned();
    let mut records = Vec::new();
    for client in fixture.clients() {
        let upload = prepare(
            &client,
            spec(fixture.worktree, "queued 🙂", b"persistent bytes"),
        )
        .await;
        stage(&client, &upload, b"persistent bytes").await;
        let request = finish(&client, &upload);
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission.completion);
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let RequestOutcome::Completed(result) = client.outcome(&request).await.unwrap() {
                    break *result;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
        .unwrap();
        let Output::Attachment(attachment) = result else {
            panic!("attachment expected")
        };
        records.push((request, attachment));
    }
    fixture.node.shutdown().await.unwrap();
    fixture.node = Node::start(&profile).await.unwrap();
    for (client, (request, attachment)) in fixture.clients().into_iter().zip(records) {
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Attachment(attachment.clone())
        );
        assert_eq!(download(&client, &attachment).await, b"persistent bytes");
    }
    fixture.close().await;
}

#[tokio::test]
async fn discards_without_replaying() {
    let mut fixture = Fixture::start().await;
    let profile = fixture.node.profile().to_owned();
    let mut records = Vec::new();
    for client in fixture.clients() {
        let upload = prepare(
            &client,
            spec(fixture.worktree, "discarded", b"private draft"),
        )
        .await;
        stage(&client, &upload, b"private draft").await;
        let request = finish(&client, &upload);
        let Output::Attachment(attachment) = client.execute(request.clone()).await.unwrap() else {
            panic!("attachment expected")
        };
        let discard = client.prepare(Command::DiscardAttachment {
            worktree: fixture.worktree,
            attachment: attachment.id,
        });
        let output = client.execute(discard.clone()).await.unwrap();
        assert_eq!(client.execute(discard.clone()).await.unwrap(), output);
        assert!(
            client
                .execute(client.prepare(read(&attachment)))
                .await
                .is_err()
        );
        assert!(
            client
                .execute(client.prepare(Command::DownloadAttachment {
                    worktree: fixture.worktree,
                    attachment: attachment.id
                }))
                .await
                .is_err()
        );
        assert!(
            profile
                .join("attachments")
                .join(attachment.id.to_string())
                .exists()
        );
        records.push((request, discard, attachment));
    }
    fixture.node.shutdown().await.unwrap();
    for (_, _, attachment) in &records {
        assert!(
            !profile
                .join("attachments")
                .join(attachment.id.to_string())
                .exists()
        );
    }
    fixture.node = Node::start(&profile).await.unwrap();
    for (client, (request, discard, attachment)) in fixture.clients().into_iter().zip(records) {
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Attachment(attachment.clone())
        );
        assert_eq!(
            client.execute(discard).await.unwrap(),
            Output::AttachmentDiscarded {
                attachment: attachment.id
            }
        );
        assert!(
            !profile
                .join("attachments")
                .join(attachment.id.to_string())
                .exists()
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn preserves_uncertain_inventory() {
    let mut fixture = Fixture::start().await;
    let profile = fixture.node.profile().to_owned();
    let [client, _] = fixture.clients();
    let upload = prepare(&client, spec(fixture.worktree, "retained", b"bytes")).await;
    stage(&client, &upload, b"bytes").await;
    let attachment = publish(&client, &upload).await;
    let orphan = profile
        .join("attachments")
        .join(AttachmentId::new().to_string());
    std::fs::write(&orphan, b"unfinished").unwrap();
    let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    database
        .execute(
            "INSERT INTO attachments(id,worktree,caller,body) VALUES('invalid',?1,?2,?3)",
            rusqlite::params![
                fixture.worktree.to_string(),
                &fixture.node.id().0[..],
                serde_json::to_vec(&attachment).unwrap()
            ],
        )
        .unwrap();
    drop(database);
    fixture.node.shutdown().await.unwrap();
    assert!(orphan.exists());
    fixture.node = Node::start(&profile).await.unwrap();
    assert!(orphan.exists());
    let [client, _] = fixture.clients();
    assert_eq!(download(&client, &attachment).await, b"bytes");
    let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    database
        .execute("DELETE FROM attachments WHERE id='invalid'", [])
        .unwrap();
    drop(database);
    fixture.controller.close().await.unwrap();
    fixture.node.shutdown().await.unwrap();
    assert!(!orphan.exists());
}
