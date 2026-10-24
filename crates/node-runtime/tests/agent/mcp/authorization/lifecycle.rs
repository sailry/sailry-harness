use super::*;
use sailry_protocol::plugin::connector::State;

pub(super) async fn stored(node: &Node, id: plugin::connector::Id) -> Vec<u8> {
    node.with_storage(move |db| {
        Box::pin(async move {
            db.prepare("SELECT body FROM plugin_connections WHERE id=?1")
                .await
                .map_err(crate::database_fixture::fault)?
                .query_row([id.to_string()])
                .await
                .map_err(crate::database_fixture::fault)?
                .get::<Vec<u8>>(0)
                .map_err(crate::database_fixture::fault)
        })
    })
    .await
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

pub(super) async fn call(fixture: &process::Fixture) -> Page {
    let (_, turn) = submit(fixture, false).await;
    let page = finish(fixture, turn.id).await;
    assert_eq!(page.runs.last().unwrap().status, Status::Completed);
    page
}

pub(super) async fn recover(fixture: &process::Fixture, peer: &input::Server, calls: usize) {
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

                let before = stored(&fixture.node, row.id).await;
                let refreshes = issuer.refreshes.load(Ordering::SeqCst);
                issuer.status.store(503, Ordering::SeqCst);
                issuer.expire();
                call(&fixture).await;
                assert!(issuer.refreshes.load(Ordering::SeqCst) > refreshes);
                assert_eq!(stored(&fixture.node, row.id).await, before);
                assert_eq!(peer.calls.load(Ordering::SeqCst), 1);
                issuer.status.store(200, Ordering::SeqCst);
                recover(&fixture, &peer, 2).await;

                let before = stored(&fixture.node, row.id).await;
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
                assert_eq!(stored(&fixture.node, row.id).await, before);
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
                    serde_json::from_slice(&stored(&fixture.node, row.id).await).unwrap();
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

#[tokio::test]
async fn reports_rejected_probe_renewals_as_expired() {
    for remote in [false, true] {
        for sse in [false, true] {
            for status in [400, 401, 503, 1] {
                let peer = input::Server::url("https://example.invalid/continue", sse).await;
                let issuer = issuer::Server::start(&peer.endpoint).await;
                let model = Server::turn_tools(Vec::new()).await;
                let fixture = process::Fixture::packages(remote, &model, &[]).await;
                configure(&fixture, &issuer.endpoint, sse, false);
                let path = fixture.root.join("package/plugin.json");
                let mut manifest: Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                let declaration = &mut manifest["extensions"][plugin::NAMESPACE]["connectors"][0];
                declaration["health"] = json!({"request":format!("{ORIGIN}/health"),"ok":[200]});
                declaration["inject"].as_array_mut().unwrap().push(json!({
                    "origin":ORIGIN,"header":"Authorization","prefix":"Bearer "
                }));
                fs::write(path, manifest.to_string()).unwrap();
                let info = install(&fixture, 0).await;
                assert!(info.issues.is_empty(), "{:?}", info.issues);
                let (_, attempt, url) =
                    begin(&fixture.client, info.summary.reference(), false).await;
                let row = complete(&fixture.client, &attempt, &url).await;
                let before = stored(&fixture.node, row.id).await;
                let revision: Value = serde_json::from_slice(&before).unwrap();
                assert_eq!(row.state, State::Connected);
                assert_eq!(revision["state"], "connected");
                assert!(!revision["grant"].is_null());
                issuer.status.store(status, Ordering::SeqCst);
                issuer.expire();
                let error = fixture
                    .client
                    .execute(
                        fixture
                            .client
                            .prepare(Command::ProbeConnection { connection: row.id }),
                    )
                    .await
                    .unwrap_err();
                let after: Value =
                    serde_json::from_slice(&stored(&fixture.node, row.id).await).unwrap();
                let rejected = matches!(status, 400 | 401);
                assert_eq!(
                    error.code,
                    if rejected {
                        ErrorCode::Expired
                    } else {
                        ErrorCode::Unavailable
                    },
                    "{remote}/{sse}/{status}: {}; state={}, grant={}, revision={}",
                    error.message,
                    after["state"],
                    !after["grant"].is_null(),
                    after["grant_revision"]
                );
                assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
                assert_eq!(issuer.refreshes.load(Ordering::SeqCst), 1);
                let Output::Connections(rows) =
                    execute(&fixture.client, Command::ListConnections).await
                else {
                    panic!("connections expected")
                };
                assert_eq!(rows.len(), 1);
                if rejected {
                    assert_eq!(rows[0].state, State::Expired);
                    assert!(after["grant"].is_null());
                    assert_eq!(
                        after["grant_revision"].as_u64().unwrap(),
                        revision["grant_revision"].as_u64().unwrap() + 1
                    );
                } else {
                    assert_eq!(rows[0].state, State::Connected);
                    assert_eq!(stored(&fixture.node, row.id).await, before);
                }
                let public = serde_json::to_string(&(error, rows)).unwrap();
                for secret in ["access-initial", "refresh-initial", "client-private-marker"] {
                    assert!(!public.contains(secret));
                }
                fixture.node.shutdown().await.unwrap();
                fixture.controller.close().await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn expires_rejected_post_rotations() {
    for remote in [false, true] {
        let peer = input::Server::url("https://example.invalid/continue", false).await;
        let issuer = issuer::Server::start(&peer.endpoint).await;
        let model = Server::turn_tools(Vec::new()).await;
        let fixture = process::Fixture::packages(remote, &model, &[]).await;
        configure(&fixture, &issuer.endpoint, false, false);
        let info = install(&fixture, 0).await;
        assert!(info.issues.is_empty(), "{:?}", info.issues);
        let initial = issuer.requests.lock().unwrap().clone();
        let (_, attempt, url) = begin(&fixture.client, info.summary.reference(), false).await;
        let waiting = issuer.requests.lock().unwrap().clone();
        let row = complete(&fixture.client, &attempt, &url).await;
        let authorized = issuer.requests.lock().unwrap().clone();
        assert!(
            authorized.iter().all(|head| head.starts_with("GET /mcp ")),
            "MCP authorization stages: initial={initial:?}, waiting={waiting:?}, authorized={authorized:?}"
        );
        let before: Value = serde_json::from_slice(&stored(&fixture.node, row.id).await).unwrap();
        let revision = before["grant_revision"].as_u64().unwrap();
        issuer.denied.store(true, Ordering::SeqCst);
        for (index, state) in [State::Connected, State::Expired].into_iter().enumerate() {
            let previous = issuer.requests.lock().unwrap().len();
            let page = call(&fixture).await;
            let Output::Connections(rows) =
                execute(&fixture.client, Command::ListConnections).await
            else {
                panic!("connections expected")
            };
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].state, state);
            let after: Value =
                serde_json::from_slice(&stored(&fixture.node, row.id).await).unwrap();
            assert_eq!(
                after["grant_revision"].as_u64().unwrap(),
                revision + 1 + index as u64
            );
            assert_eq!(after["grant"].is_null(), index == 1);
            assert_eq!(issuer.refreshes.load(Ordering::SeqCst), 1);
            let requests = issuer.requests.lock().unwrap().clone();
            let operation = &requests[previous..];
            assert_eq!(
                operation
                    .iter()
                    .filter(|head| head.starts_with("POST /mcp "))
                    .count(),
                1,
                "POST must not be replayed: {operation:?}"
            );
            assert_eq!(
                requests
                    .iter()
                    .filter(|head| head.starts_with("POST /mcp "))
                    .count(),
                index + 1
            );
            assert!(
                operation.iter().all(|head| {
                    head.starts_with("GET /mcp ") || head.starts_with("POST /mcp ")
                })
            );
            assert_eq!(peer.calls.load(Ordering::SeqCst), 0);
            let public = serde_json::to_string(&(page, rows)).unwrap();
            for marker in ["access-initial", "refresh-initial", "access-1", "refresh-1"] {
                assert!(!public.contains(marker));
            }
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
