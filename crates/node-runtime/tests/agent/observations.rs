//! Request observations survive terminal delivery and cancellation.
use super::*;
use sailry_protocol::conversation::observation::Outcome;

pub(super) async fn statistics(client: &Client, session: SessionId) -> Statistics {
    let mut stream = client.subscribe_conversation(session).await.unwrap();
    let mut projection = sailry_client::conversation::Projection::new(client.target(), session, 0);
    loop {
        projection.apply(0, stream.next().await.unwrap()).unwrap();
        if let Some(snapshot) = projection.snapshot() {
            return snapshot.statistics.clone();
        }
    }
}

#[tokio::test]
async fn finalizes_terminal_and_cancelled_streams() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        for cancel in [false, true] {
            let root = directory.path().join(if cancel {
                "cancelled-project"
            } else {
                "completed-project"
            });
            std::fs::create_dir(&root).unwrap();
            let server = Server::start(cancel).await;
            let (session, _) = configured_session(&client, &server.endpoint, &root).await;
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: "PRIVATE_REQUEST_MARKER".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            if cancel {
                tokio::time::timeout(Duration::from_secs(5), async {
                    while server.requests.lock().unwrap().is_empty() {
                        tokio::task::yield_now().await;
                    }
                    client
                        .execute(client.prepare(Command::StopTurn { turn: turn.id }))
                        .await
                        .unwrap();
                })
                .await
                .unwrap();
            }
            let page = finished(&client, session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                if cancel {
                    Status::Cancelled
                } else {
                    Status::Completed
                }
            );
            assert!(
                page.entries
                    .iter()
                    .filter(|entry| !is_context(entry))
                    .all(|entry| !entry.parts.is_empty())
            );
            let stats = statistics(&client, session.id).await;
            let observations = &stats.observations;
            assert_eq!(observations.calls, 1);
            let call = observations.recent.last().unwrap();
            let attempt = call.attempts.last().unwrap();
            assert_eq!(
                attempt.outcome,
                if cancel {
                    Outcome::Cancelled
                } else {
                    Outcome::Succeeded
                }
            );
            assert!(attempt.elapsed_us.is_some());
            if !cancel {
                assert_eq!(attempt.transport, "gdk");
                assert_eq!(attempt.status, None);
                assert_eq!(attempt.response_us, None);
                assert_eq!(attempt.headers_us, None);
                assert!(attempt.first_text_us.is_some());
            }
            assert_eq!(call.boundary, None);
            assert!(
                !serde_json::to_string(observations)
                    .unwrap()
                    .contains("PRIVATE_REQUEST_MARKER")
            );
            assert_eq!(
                statistics(&client, session.id).await,
                stats,
                "re-reading does not duplicate attempts"
            );
            // A newly subscribed controller receives the same canonical projection.
            let mut updates = client.subscribe_conversation(session.id).await.unwrap();
            let mut projection =
                sailry_client::conversation::Projection::new(client.target(), session.id, 0);
            loop {
                projection.apply(0, updates.next().await.unwrap()).unwrap();
                if let Some(snapshot) = projection.snapshot() {
                    assert!(
                        snapshot
                            .page
                            .entries
                            .iter()
                            .filter(|entry| !is_context(entry))
                            .all(|entry| !entry.parts.is_empty())
                    );
                    assert_eq!(snapshot.statistics.observations, *observations);
                    break;
                }
            }
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
