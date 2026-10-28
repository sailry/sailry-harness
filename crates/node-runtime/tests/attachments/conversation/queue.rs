use super::*;

#[tokio::test]
async fn preserves_images_after_guidance() {
    for remote in [false, true] {
        let fixture = Fixture::empty().await;
        let client = fixture
            .clients()
            .into_iter()
            .nth(usize::from(remote))
            .unwrap();
        let release = CancellationToken::new();
        let server = server::Server::markdown_held("Image received".into(), release.clone()).await;
        let (session, _) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let bytes = png();
        let first = upload(&client, fixture.worktree, "first.png", "image/png", &bytes).await;
        let active = queue(&client, &session, input("Inspect this image", &[&first])).await;
        client
            .execute(client.prepare(Command::StartQueuedTurn { turn: active.id }))
            .await
            .unwrap();
        server.wait_count(1).await;
        let selected = queue(&client, &session, "Continue with the same image".into()).await;
        client
            .execute(client.prepare(Command::GuideQueuedTurn {
                turn: selected.id,
                recipient: active.id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        release.cancel();
        server.wait_count(2).await;
        let history = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let history = page(&client, session.id).await;
                if history.runs.iter().any(|run| {
                    run.turn == active.id
                        && !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                }) {
                    return history;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("guided attachment turn deadline");
        assert_eq!(
            history
                .runs
                .iter()
                .find(|run| run.turn == active.id)
                .unwrap()
                .status,
            Status::Completed,
            "{:?}",
            history.runs
        );
        let joined = history
            .runs
            .iter()
            .find(|run| run.turn == selected.id)
            .unwrap();
        assert_eq!(joined.status, Status::Continued);
        assert_eq!(joined.joined, Some(active.id));
        assert_eq!(
            history
                .entries
                .iter()
                .filter(|entry| entry.turn == selected.id
                    && entry.parts == vec![Part::Text("Continue with the same image".into())])
                .count(),
            1
        );
        let second = upload(&client, fixture.worktree, "second.png", "image/png", &bytes).await;
        let next = queue(
            &client,
            &session,
            input("Inspect another image", &[&second]),
        )
        .await;
        let history = run(&client, &next).await;
        assert_eq!(
            history.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            history.runs
        );
        for (turn, attachment) in [(active.id, &first), (next.id, &second)] {
            assert_eq!(
                history
                    .entries
                    .iter()
                    .filter(|entry| entry.turn == turn)
                    .flat_map(|entry| &entry.parts)
                    .filter(|part| **part == Part::Attachment(attachment.clone()))
                    .count(),
                1
            );
            assert_eq!(download(&client, attachment).await, bytes);
        }
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .map(|request| request
                    .to_string()
                    .matches("data:image/png;base64,")
                    .count())
                .collect::<Vec<_>>(),
            vec![1, 1, 2]
        );
        assert!(
            requests
                .iter()
                .all(|request| !request.to_string().contains("sailry-attachment://"))
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn freezes_native_input_support() {
    let fixture = Fixture::empty().await;
    let server = server::Server::start(false).await;
    for client in fixture.clients() {
        let (session, mut provider) = session(
            &client,
            fixture.worktree,
            &server.endpoint,
            ModelApi::ChatCompletions,
        )
        .await;
        let image = upload(
            &client,
            fixture.worktree,
            "preview.png",
            "image/png",
            &png(),
        )
        .await;
        let turn = queue(&client, &session, input("Describe the image", &[&image])).await;
        provider.models[0]
            .capabilities
            .get_or_insert_with(Default::default)
            .vision = Some(false);
        provider.models[0].capabilities.as_mut().unwrap().tools = Some(true);
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
        assert!(pair[0].to_string().contains("data:image/png;base64,"));
        assert!(!pair[0].to_string().contains("Contents not included"));
        assert!(!pair[1].to_string().contains("data:image/png;base64,"));
        assert!(
            pair[1]
                .to_string()
                .contains("Contents not included; use read_office")
        );
        assert!(
            pair[1]
                .to_string()
                .contains("use an attachment-capable tool")
        );
    }
    fixture.close().await;
}

#[tokio::test]
async fn retains_revisions() {
    let mut fixture = Fixture::empty().await;
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
        provider.models[0]
            .capabilities
            .get_or_insert_with(Default::default)
            .vision = Some(false);
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
    fixture.node = Node::start_empty(&profile).await.unwrap();
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
    let fixture = Fixture::empty().await;
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
        provider.models[0]
            .capabilities
            .get_or_insert_with(Default::default)
            .vision = Some(false);
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
