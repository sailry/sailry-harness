use super::*;

#[tokio::test]
async fn preserves_early_failure_input_across_clients() {
    for remote in [false, true] {
        let mut fixture = Fixture::start(remote, false).await;
        // An absent provider fails before Runner writes a canonical user event.
        let mut config = fixture.session.config.clone();
        config.provider = ProviderId::new();
        config.credential = None;
        let Output::Session(session) = fixture
            .client
            .execute(fixture.client.prepare(Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        fixture.session = session;
        fixture.pause(true).await;
        let turn = fixture.enqueue("Original queued input").await;
        let input: Input = "Effective edited input 中文 🙂\n".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::EditQueuedTurn {
                turn: turn.id,
                expected_revision: 1,
                message: input.clone(),
            }))
            .await
            .unwrap();
        let mut stream = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let mut projection = Projection::new(fixture.node.id(), fixture.session.id, 1);
        projection.apply(1, stream.next().await.unwrap()).unwrap();
        assert!(
            projection
                .snapshot()
                .unwrap()
                .page
                .runs
                .iter()
                .all(|run| run.input.is_none())
        );
        fixture.pause(false).await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                projection.apply(1, stream.next().await.unwrap()).unwrap();
                if projection
                    .snapshot()
                    .unwrap()
                    .page
                    .runs
                    .iter()
                    .any(|run| run.turn == turn.id && run.status == Status::Failed)
                {
                    break;
                }
            }
        })
        .await
        .unwrap();
        let page = &projection.snapshot().unwrap().page;
        assert!(page.entries.is_empty());
        assert_eq!(
            sailry_client::conversation::turns::input(page, turn.id)
                .unwrap()
                .message,
            input
        );
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(snapshot.sessions.iter().all(|session| {
            session
                .activity
                .run
                .as_ref()
                .is_none_or(|run| run.input.is_none())
        }));
        drop(stream);
        let fixture = fixture.restart().await;
        let recovered = fixture
            .client
            .read_conversation(fixture.session.id, None, 100)
            .await
            .unwrap();
        assert_eq!(
            sailry_client::conversation::turns::input(&recovered.page, turn.id)
                .unwrap()
                .message,
            input
        );
        assert!(recovered.page.entries.is_empty());
        let request = fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: sailry_client::conversation::turns::input(&recovered.page, turn.id)
                .unwrap()
                .message,
        });
        let receipt = fixture.client.execute(request.clone()).await.unwrap();
        assert_eq!(fixture.client.execute(request).await.unwrap(), receipt);
        let Output::QueuedTurn(retry) = receipt else {
            panic!("turn expected")
        };
        assert_ne!(retry.id, turn.id);
        let page = finished(&fixture.client, fixture.session.id, retry.id).await;
        assert!(page.entries.is_empty());
        for id in [turn.id, retry.id] {
            assert_eq!(
                sailry_client::conversation::turns::input(&page, id)
                    .unwrap()
                    .message,
                input
            );
        }
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        fixture.close().await;
    }
}
