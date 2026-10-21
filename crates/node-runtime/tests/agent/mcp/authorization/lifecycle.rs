use super::*;
use sailry_protocol::plugin::connector::State;

fn stored(node: &Node, id: plugin::connector::Id) -> Vec<u8> {
    rusqlite::Connection::open_with_flags(
        node.profile().join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row(
        "SELECT body FROM plugin_connections WHERE id=?1",
        [id.to_string()],
        |row| row.get(0),
    )
    .unwrap()
}

async fn finish(fixture: &process::Fixture, turn: TurnId) -> Page {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let page = history(&fixture.client, fixture.session.id).await;
            for question in page
                .questions
                .iter()
                .filter(|question| question.state == question::State::Pending)
            {
                execute(
                    &fixture.client,
                    Command::ResolveQuestion {
                        session: fixture.session.id,
                        question: question.id,
                        response: question::Response::Decline,
                    },
                )
                .await;
            }
            if page.runs.iter().any(|run| {
                run.turn == turn
                    && matches!(
                        run.status,
                        Status::Completed | Status::Failed | Status::Cancelled
                    )
            }) {
                return page;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("MCP refresh deadline")
}

async fn call(fixture: &process::Fixture) -> Page {
    let (_, turn) = submit(fixture, false).await;
    let page = finish(fixture, turn.id).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    page
}

async fn recover(fixture: &process::Fixture, peer: &input::Server, calls: usize) {
    // A failed POST can refresh credentials without replaying initialization.
    // Each turn here is a separate explicit request using the persisted grant.
    for _ in 0..2 {
        call(fixture).await;
        if peer.calls.load(Ordering::SeqCst) == calls {
            return;
        }
    }
    assert_eq!(peer.calls.load(Ordering::SeqCst), calls);
}

#[tokio::test]
async fn preserves_and_rejects_refresh_credentials() {
    for remote in [false, true] {
        for sse in [false, true] {
            for rejection in [401, 400] {
                let peer = input::Server::url("https://example.invalid/continue", sse).await;
                let issuer = issuer::Server::start(&peer.endpoint).await;
                let model = Server::turn_tools(vec![(alias("input", "read"), json!({}))]).await;
                let fixture = process::Fixture::packages(remote, &model, &[]).await;
                configure(&fixture, &issuer.endpoint, sse, false);
                let info = install(&fixture, 0).await;
                assert!(info.issues.is_empty(), "{:?}", info.issues);
                let (_, attempt, url) =
                    begin(&fixture.client, info.summary.reference(), false).await;
                let row = complete(&fixture.client, &attempt, &url).await;
                call(&fixture).await;
                assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
                let Output::Connection(saved) = execute(
                    &fixture.client,
                    Command::UpdateConnection {
                        connection: row.id,
                        expected_revision: row.revision,
                        input: Default::default(),
                    },
                )
                .await
                else {
                    panic!("connection expected")
                };
                assert_eq!(saved.revision, row.revision + 1);
                assert_eq!(saved.state, State::Connected);

                let before = stored(&fixture.node, row.id);
                let refreshes = issuer.refreshes.load(Ordering::SeqCst);
                issuer.status.store(503, Ordering::SeqCst);
                issuer.expire();
                call(&fixture).await;
                assert!(issuer.refreshes.load(Ordering::SeqCst) > refreshes);
                assert_eq!(stored(&fixture.node, row.id), before);
                assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
                issuer.status.store(200, Ordering::SeqCst);
                recover(&fixture, &peer, 2).await;

                let before = stored(&fixture.node, row.id);
                issuer.status.store(0, Ordering::SeqCst);
                issuer.expire();
                let refreshes = issuer.refreshes.load(Ordering::SeqCst);
                let (_, turn) = submit(&fixture, false).await;
                tokio::time::timeout(Duration::from_secs(5), async {
                    while issuer.refreshes.load(Ordering::SeqCst) == refreshes {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
                .unwrap();
                execute(&fixture.client, Command::StopTurn { turn: turn.id }).await;
                let page = finish(&fixture, turn.id).await;
                assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
                assert_eq!(stored(&fixture.node, row.id), before);
                issuer.status.store(200, Ordering::SeqCst);
                recover(&fixture, &peer, 3).await;

                issuer.status.store(rejection, Ordering::SeqCst);
                issuer.expire();
                let refreshes = issuer.refreshes.load(Ordering::SeqCst);
                let page = call(&fixture).await;
                assert!(issuer.refreshes.load(Ordering::SeqCst) > refreshes);
                assert_eq!(peer.calls.load(Ordering::SeqCst), 3);
                let Output::Connections(rows) =
                    execute(&fixture.client, Command::ListConnections).await
                else {
                    panic!("connections expected")
                };
                assert_eq!(rows[0].state, State::Expired);
                let rejected: Value =
                    serde_json::from_slice(&stored(&fixture.node, row.id)).unwrap();
                assert!(rejected["grant"].is_null());
                for marker in ["access-initial", "refresh-initial", "client-private-marker"] {
                    assert!(!serde_json::to_string(&page).unwrap().contains(marker));
                }
                fixture.node.shutdown().await.unwrap();
                fixture.controller.close().await.unwrap();
            }
        }
    }
}
