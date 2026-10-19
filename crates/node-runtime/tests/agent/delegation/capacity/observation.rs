use super::*;

#[tokio::test]
async fn preserves_admitted_work_after_wait_expiry() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let child = Server::markdown_held("Deferred child answer".into(), release.clone()).await;
        let fixture = Fixture::start(remote, false, child, 3).await;
        fixture.capacity(1).await;
        fixture.observation(Duration::from_millis(100));
        let parent = fixture.push(0).await;
        let page = finished(&fixture.client, fixture.parents[0].id, parent.id).await;
        assert_eq!(status(&page, parent.id), Status::Completed);
        let values = results(&page);
        assert_eq!(values.len(), 3);
        let runs = fixture.wait(3, 1).await;
        for value in &values {
            let run = runs
                .iter()
                .find(|run| value["turn"] == run.turn.to_string())
                .unwrap();
            assert_eq!(value["admitted"], true);
            assert_eq!(value["pending"], true);
            assert_eq!(value["session"], run.session.to_string());
            assert_eq!(value["turn"], run.turn.to_string());
            if run.status == Status::Queued {
                assert_eq!(value["status"], "queued");
            } else {
                assert!(value["status"] == "queued" || value["status"] == "running");
            }
            assert!(value.get("isError").is_none());
            assert!(value.get("error").is_none());
        }
        assert_eq!(runs[0].status, Status::Running);
        assert!(runs[1..].iter().all(|run| run.status == Status::Queued));
        fixture.child.wait_count(1).await;
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        let _waiter = fixture.waiter(&runs[0]).await;
        let waiting = fixture.push(0).await;
        let page = finished(&fixture.client, fixture.parents[0].id, waiting.id).await;
        assert_eq!(status(&page, waiting.id), Status::Completed);
        let values = results(&page);
        let value = values.last().unwrap();
        assert_eq!(value["admitted"], true);
        assert_eq!(value["pending"], true);
        assert_eq!(value["session"], runs[0].session.to_string());
        assert_eq!(value["turn"], runs[0].turn.to_string());
        assert_eq!(value["status"], "running");
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        let captures = futures::future::try_join_all(
            runs.iter()
                .map(|run| fixture.client.read_turn_capture(run.session, run.turn)),
        )
        .await
        .unwrap();
        release.cancel();
        for run in &runs {
            assert_eq!(
                status(
                    &finished(&fixture.client, run.session, run.turn).await,
                    run.turn
                ),
                Status::Completed
            );
        }
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 3);
        for capture in captures {
            assert_eq!(
                fixture
                    .client
                    .read_turn_capture(capture.session, capture.id)
                    .await
                    .unwrap(),
                capture
            );
        }
        // Completion uses the existing automatic continuation, not another spawn.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&fixture.client, fixture.parents[0].id).await;
                if page
                    .runs
                    .iter()
                    .any(|run| run.notice && run.status == Status::Completed)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("completion continuation deadline");
        let collected = fixture.push(0).await;
        let page = finished(&fixture.client, fixture.parents[0].id, collected.id).await;
        assert_eq!(status(&page, collected.id), Status::Completed);
        let value = results(&page).last().unwrap().clone();
        assert_eq!(value["status"], "completed");
        assert_eq!(value["response"], "Deferred child answer");
        assert!(value.get("pending").is_none());
        assert_eq!(fixture.runs().await.len(), 3);
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 3);
        fixture.close().await;
    }
}

#[tokio::test]
async fn stops_deferred_work_without_replay() {
    for remote in [false, true] {
        let release = CancellationToken::new();
        let child = Server::markdown_held("Deferred child answer".into(), release).await;
        let mut fixture = Fixture::start(remote, false, child, 3).await;
        fixture.capacity(1).await;
        fixture.observation(Duration::from_millis(100));
        let parent = fixture.push(0).await;
        let page = finished(&fixture.client, fixture.parents[0].id, parent.id).await;
        assert_eq!(status(&page, parent.id), Status::Completed);
        let runs = fixture.wait(3, 1).await;
        fixture.child.wait_count(1).await;
        let queued = &runs[1];
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: queued.turn }),
            )
            .await
            .unwrap();
        assert_eq!(
            status(
                &finished(&fixture.client, queued.session, queued.turn).await,
                queued.turn
            ),
            Status::Cancelled
        );
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        fixture = fixture.restart().await;
        for run in fixture.runs().await {
            assert_eq!(
                run.status,
                if run.turn == queued.turn {
                    Status::Cancelled
                } else {
                    Status::Interrupted
                }
            );
        }
        assert_eq!(fixture.child.requests.lock().unwrap().len(), 1);
        fixture.close().await;
    }
}
