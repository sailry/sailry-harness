use super::*;

#[tokio::test]
async fn freezes_native_input_support() {
    let fixture = Fixture::start().await;
    let server = server::Server::start(false).await;
    for client in fixture.clients() {
        let (session, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let pdf = upload(
            &client,
            fixture.worktree,
            "report.txt",
            "text/plain",
            &pdf(),
        )
        .await;
        let turn = queue(&client, &session, input("Read the PDF", &[&pdf])).await;
        provider.models[0].vision = false;
        provider.models[0].tools = true;
        client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        assert_eq!(run(&client, &turn).await.runs[0].status, Status::Completed);
        let next = queue(&client, &session, "Read it again".into()).await;
        assert_eq!(
            run(&client, &next).await.runs.last().unwrap().status,
            Status::Completed
        );
    }
    let requests = server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 4);
    for pair in requests.as_chunks::<2>().0 {
        assert!(pair[0].to_string().contains("data:application/pdf;base64,"));
        assert!(!pair[0].to_string().contains("Contents not included"));
        assert!(!pair[1].to_string().contains("data:application/pdf;base64,"));
        assert!(
            pair[1]
                .to_string()
                .contains("Contents not included; an attachment-capable tool is required")
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn retains_revisions() {
    let mut fixture = Fixture::start().await;
    let server = server::Server::start(false).await;
    let mut records = Vec::new();
    for client in fixture.clients() {
        let (session, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let first = upload(
            &client,
            fixture.worktree,
            "first.txt",
            "text/plain",
            b"first input",
        )
        .await;
        let second = upload(
            &client,
            fixture.worktree,
            "second.png",
            "image/png",
            b"image fixture",
        )
        .await;
        let turn = queue(&client, &session, input("", &[&first])).await;
        assert_eq!(
            page(&client, session.id).await.queue.items[0].attachments,
            vec![first.clone()]
        );
        assert_eq!(
            client
                .execute(discard(&client, &first))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        provider.models[0].vision = false;
        client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let edit = client.prepare(Command::EditQueuedTurn {
            turn: turn.id,
            expected_revision: 1,
            message: input("edited", &[&second]),
        });
        let edited = client.execute(edit.clone()).await.unwrap();
        assert_eq!(client.execute(edit.clone()).await.unwrap(), edited);
        let stale = client.prepare(Command::EditQueuedTurn {
            turn: turn.id,
            expected_revision: 1,
            message: input("stale", &[&first]),
        });
        assert_eq!(
            client.execute(stale).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        client.execute(discard(&client, &first)).await.unwrap();
        assert_eq!(
            client
                .execute(discard(&client, &second))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        records.push((turn, second, edit, edited));
    }
    let profile = fixture.node.profile().to_owned();
    fixture.node.shutdown().await.unwrap();
    fixture.node = Node::start(&profile).await.unwrap();
    for (client, (turn, attachment, edit, edited)) in fixture.clients().into_iter().zip(records) {
        assert_eq!(client.execute(edit).await.unwrap(), edited);
        let Output::QueuedMessage(message) = client
            .execute(client.prepare(Command::ReadQueuedTurn { turn: turn.id }))
            .await
            .unwrap()
        else {
            panic!("input expected")
        };
        assert_eq!(message.message, input("edited", &[&attachment]));
        assert_eq!(
            page(&client, turn.session).await.queue.items[0].attachments,
            vec![attachment.clone()]
        );
        client
            .execute(client.prepare(Command::RemoveQueuedTurn {
                turn: turn.id,
                expected_revision: 2,
            }))
            .await
            .unwrap();
        client.execute(discard(&client, &attachment)).await.unwrap();
    }
    assert!(server.requests.lock().unwrap().is_empty());
    fixture.close().await;
}

#[tokio::test]
async fn rejects_invalid_inputs() {
    let fixture = Fixture::start().await;
    let server = server::Server::start(false).await;
    for client in fixture.clients() {
        let (session, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let text = upload(&client, fixture.worktree, "text", "text/plain", b"valid").await;
        let unsupported = upload(
            &client,
            fixture.worktree,
            "binary",
            "application/octet-stream",
            b"binary",
        )
        .await;
        let image = upload(&client, fixture.worktree, "image", "image/png", b"image").await;
        let other = tempfile::tempdir().unwrap();
        let worktree = files::register(&client, other.path()).await;
        let foreign = upload(&client, worktree, "foreign", "text/plain", b"foreign").await;
        for message in [
            Input::default(),
            input("", &[&text, &text]),
            input("", &[&foreign]),
            Input {
                references: Vec::new(),
                text: "invalid".into(),
                attachments: vec![AttachmentId::new()],
            },
            Input {
                references: Vec::new(),
                text: "invalid".into(),
                attachments: (0..9).map(|_| AttachmentId::new()).collect(),
            },
        ] {
            assert!(
                client
                    .execute(client.prepare(Command::SubmitTurn {
                        session: session.id,
                        expected_revision: 1,
                        message
                    }))
                    .await
                    .is_err()
            );
        }
        provider.models[0].vision = false;
        client
            .execute(client.prepare(Command::PutProvider {
                provider,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        queue(&client, &session, input("", &[&image, &unsupported])).await;
        let history = page(&client, session.id).await;
        assert!(history.entries.is_empty());
        assert_eq!(history.queue.items[0].attachments, vec![image, unsupported]);
        client.execute(discard(&client, &text)).await.unwrap();
    }
    assert!(server.requests.lock().unwrap().is_empty());
    fixture.close().await;
}
