use super::*;

#[tokio::test]
async fn restart_pauses_idle_input() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote, false).await;
        let input = "Preserve queued input 中文 🙂\n".repeat(100);
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: input.clone().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected")
        };
        let queued = read(&fixture.client, turn.id).await;
        let before = fixture.queue().await;
        assert!(!before.paused);
        assert!(!before.items[0].ready);
        let fixture = fixture.restart().await;
        let recovered = fixture.queue().await;
        assert!(recovered.paused);
        assert_eq!(recovered.revision, before.revision + 1);
        assert_eq!(recovered.items, before.items);
        assert_eq!(read(&fixture.client, turn.id).await, queued);
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StartQueuedTurn { turn: turn.id }),
            )
            .await
            .unwrap();
        assert!(fixture.queue().await.paused);
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        let fresh = fixture.push("Resume with fresh input").await;
        let page = finished(&fixture.client, fixture.session.id, fresh.id).await;
        assert!(!page.queue.paused);
        assert!(page.runs.iter().all(|run| run.status == Status::Completed));
        let requests = fixture.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(prompt(&requests[0]), input);
        assert_eq!(prompt(&requests[1]), "Resume with fresh input");
        drop(requests);
        fixture.close().await;
    }
}

#[tokio::test]
async fn fresh_submission_resumes_active_queue() {
    use sailry_link::CancellationToken;

    for remote in [false, true] {
        let release = CancellationToken::new();
        let server = Server::markdown_held("Natural answer".into(), release.clone()).await;
        let fixture = Fixture::with_server(remote, server).await;
        let active = fixture.push("Active task").await;
        fixture.server.wait_count(1).await;
        fixture.pause(true).await;
        let waiting = fixture.enqueue("Waiting input").await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::EditQueuedTurn {
                turn: waiting.id,
                expected_revision: 1,
                message: "Edited waiting input".into(),
            }))
            .await
            .unwrap();
        let queue = fixture.queue().await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::MoveQueuedTurn {
                session: fixture.session.id,
                expected_revision: queue.revision,
                turn: waiting.id,
                before: None,
            }))
            .await
            .unwrap();
        assert!(fixture.queue().await.paused);
        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Fresh input".into(),
        });
        let Output::QueuedTurn(fresh) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("queued turn expected")
        };
        let resumed = fixture.queue().await;
        assert!(!resumed.paused);
        assert_eq!(
            resumed
                .items
                .iter()
                .map(|item| item.turn)
                .collect::<Vec<_>>(),
            [waiting.id, fresh.id]
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);

        // Replaying the old admission receipt must not resume a later pause.
        fixture.pause(true).await;
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(fresh.clone())
        );
        assert!(fixture.queue().await.paused);
        fixture
            .client
            .execute(fixture.client.prepare(Command::SendQueuedTurn {
                turn: fresh.id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        fixture.server.wait_count(2).await;
        assert!(!fixture.queue().await.paused);
        release.cancel();
        let page = finished(&fixture.client, fixture.session.id, waiting.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == active.id)
                .unwrap()
                .status,
            Status::Cancelled
        );
        assert!(page.queue.items.is_empty());
        let requests = fixture.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(prompt(&requests[1]), "Fresh input");
        assert_eq!(prompt(&requests[2]), "Edited waiting input");
        drop(requests);
        fixture.close().await;
    }
}

#[tokio::test]
async fn restores_execution_timestamps() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote, false).await;
        fixture.pause(true).await;
        let turn = fixture.enqueue("Timed fixture").await;
        let page = history(&fixture.client, fixture.session.id).await;
        let queued = page.runs.iter().find(|run| run.turn == turn.id).unwrap();
        assert_eq!(queued.started_ms, None);
        assert_eq!(queued.finished_ms, None);
        let before = chrono::Utc::now().timestamp_millis();
        fixture.pause(false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        let completed = page
            .runs
            .iter()
            .find(|run| run.turn == turn.id)
            .unwrap()
            .clone();
        assert!(completed.started_ms.unwrap() >= before);
        assert!(completed.finished_ms.unwrap() >= completed.started_ms.unwrap());
        let fixture = fixture.restart().await;
        let restored = history(&fixture.client, fixture.session.id).await;
        assert_eq!(
            restored
                .runs
                .iter()
                .find(|run| run.turn == turn.id)
                .unwrap(),
            &completed
        );
        fixture.close().await;
    }
}

#[tokio::test]
async fn shutdown_pauses_remaining_input() {
    for remote in [false, true] {
        let fixture = Fixture::start(remote, true).await;
        let active = fixture.push("active").await;
        fixture.server.wait_count(1).await;
        let quick = Server::start(false).await;
        let mut provider = fixture.provider.clone();
        provider.endpoint = quick.endpoint.clone();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                provider,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        let waiting = fixture.push("waiting").await;
        let fixture = fixture.restart().await;
        let page = history(&fixture.client, fixture.session.id).await;
        assert!(page.queue.paused);
        assert_eq!(page.queue.items[0].turn, waiting.id);
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == active.id)
                .unwrap()
                .status,
            Status::Interrupted
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        assert!(quick.requests.lock().unwrap().is_empty());
        fixture.pause(false).await;
        let page = finished(&fixture.client, fixture.session.id, waiting.id).await;
        assert_eq!(
            page.runs
                .iter()
                .find(|run| run.turn == waiting.id)
                .unwrap()
                .status,
            Status::Completed
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        assert_eq!(quick.requests.lock().unwrap().len(), 1);
        fixture.close().await;
    }
}

#[tokio::test]
async fn preserves_input_beyond_previews() {
    let fixture = Fixture::start(false, false).await;
    for index in 0..256 {
        fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: format!("message-{index}").into(),
            }))
            .await
            .unwrap();
    }
    let page = history(&fixture.client, fixture.session.id).await;
    assert_eq!(page.runs.len(), 100);
    assert_eq!(page.queue.items.len(), 256);
    assert_eq!(page.queue.items[0].preview, "message-0");
    assert_eq!(
        fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: 1,
                message: "overflow".into(),
            }))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(fixture.queue().await, page.queue);
    let turn = page.queue.items[0].turn;
    let complete = "a".repeat(1024 * 1024);
    fixture
        .client
        .execute(fixture.client.prepare(Command::EditQueuedTurn {
            turn,
            expected_revision: 1,
            message: complete.clone().into(),
        }))
        .await
        .unwrap();
    assert_eq!(read(&fixture.client, turn).await.message.text, complete);
    let queue = fixture.queue().await;
    assert_eq!(queue.items[0].preview.len(), 256);
    assert!(queue.items[0].truncated);
    assert!(serde_json::to_vec(&queue).unwrap().len() < 512 * 1024);
    assert_eq!(
        fixture
            .client
            .execute(fixture.client.prepare(Command::EditQueuedTurn {
                turn,
                expected_revision: 2,
                message: format!("{complete}b").into(),
            }))
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    let unicode = "中文\0🙂".repeat(100);
    fixture
        .client
        .execute(fixture.client.prepare(Command::EditQueuedTurn {
            turn,
            expected_revision: 2,
            message: unicode.clone().into(),
        }))
        .await
        .unwrap();
    let queue = fixture.queue().await;
    assert_eq!(
        queue.items[0].preview,
        unicode.chars().take(256).collect::<String>()
    );
    assert!(queue.items[0].truncated);
    assert_eq!(read(&fixture.client, turn).await.message.text, unicode);
    assert!(fixture.server.requests.lock().unwrap().is_empty());
    fixture.close().await;
}
