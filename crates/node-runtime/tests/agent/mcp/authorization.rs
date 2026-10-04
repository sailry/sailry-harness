use super::*;
use sailry_protocol::plugin::authorization::{Attempt, State};
use std::sync::atomic::Ordering;
#[path = "../../support/mcp_oauth.rs"]
mod issuer;

async fn state(client: &Client, attempt: &Attempt, ready: impl Fn(&State) -> bool) -> State {
    let mut stream = client.subscribe_mcp_login(attempt.id).await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let Update::McpLogin(update) = stream.next().await.unwrap() else {
                panic!("login update expected")
            };
            if ready(&update.state) {
                return update.state;
            }
            assert!(
                update.state.active(),
                "unexpected state: {:?}",
                update.state
            );
        }
    })
    .await
    .expect("authorization deadline")
}

async fn begin(client: &Client, package: plugin::Reference) -> (Request, Attempt, String) {
    let request = client.prepare(Command::BeginMcpLogin {
        package,
        server: "input".into(),
        redirect: "http://127.0.0.1:43191/callback".into(),
        client_id: None,
    });
    let Output::McpLogin(attempt) = client.execute(request.clone()).await.unwrap() else {
        panic!("attempt expected")
    };
    let State::Pending { url, .. } =
        state(client, &attempt, |s| matches!(s, State::Pending { .. })).await
    else {
        unreachable!()
    };
    (request, attempt, url)
}

async fn setup(
    remote: bool,
    sse: bool,
) -> (
    process::Fixture,
    Server,
    input::Server,
    issuer::Server,
    plugin::Reference,
) {
    let input = input::Server::with_transport(input::form_schema(), sse).await;
    let issuer = issuer::Server::start(&input.endpoint).await;
    let model = Server::tools(vec![(alias("input", "read"), json!({}))]).await;
    let fixture = process::Fixture::new(remote, &model).await;
    package(
        &fixture.root,
        json!({"input":{"type": if sse {"sse"} else {"streamable-http"}, "url":issuer.endpoint}}),
        "1.0.0",
    );
    let package = install(&fixture, 0).await.summary.reference();
    (fixture, model, input, issuer, package)
}

#[tokio::test]
async fn resumes_authorized_tools() {
    for remote in [false, true] {
        for sse in [false, true] {
            let (fixture, model, input, issuer, package) = setup(remote, sse).await;
            let (begin_request, attempt, url) = begin(&fixture.client, package.clone()).await;
            let other = Client::new(if remote {
                fixture.node.local()
            } else {
                fixture
                    .controller
                    .handle()
                    .remote(fixture.node.link().address())
            });
            let denied = match other.subscribe_mcp_login(attempt.id).await {
                Ok(mut stream) => stream.next().await.unwrap_err(),
                Err(error) => error,
            };
            assert_eq!(denied.code, ErrorCode::NotFound);
            fixture
                .controller
                .handle()
                .disconnect(fixture.node.id())
                .await;
            let callback = issuer::Server::callback(&url).await;
            let complete = fixture.client.prepare(Command::CompleteMcpLogin {
                attempt: attempt.id,
                callback: Secret::new(callback.clone()),
            });
            let receipt = fixture.client.execute(complete.clone()).await.unwrap();
            let State::Connected(connected) = state(&fixture.client, &attempt, |s| {
                matches!(s, State::Connected(_))
            })
            .await
            else {
                unreachable!()
            };
            assert_ne!(package, connected);
            let Output::McpAuthorization(status) = execute(
                &fixture.client,
                Command::ReadMcpAuthorization {
                    package: connected.clone(),
                    server: "input".into(),
                },
            )
            .await
            else {
                panic!("status expected")
            };
            assert!(status.configured);

            let (request, turn) = submit(&fixture, false).await;
            let (_, pending) = questions::pending(&fixture.client, fixture.session.id).await;
            // Force an HTTP 401 during a long tool call. The SDK refreshes the same grant.
            issuer.expire();
            execute(
                &fixture.client,
                Command::ResolveQuestion {
                    session: fixture.session.id,
                    question: pending.id,
                    response: question::Response::Decline,
                },
            )
            .await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            assert_eq!(input.calls.load(Ordering::SeqCst), 1);
            assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
            assert_eq!(issuer.refreshes.load(Ordering::SeqCst), 1);
            assert_eq!(
                fixture.client.execute(complete.clone()).await.unwrap(),
                receipt
            );
            assert_eq!(
                fixture.client.execute(begin_request.clone()).await.unwrap(),
                Output::McpLogin(attempt.clone())
            );
            let public = serde_json::to_string(&(
                page.clone(),
                model.requests.lock().unwrap().clone(),
                status,
            ))
            .unwrap();
            for secret in [
                "access-initial",
                "refresh-initial",
                "access-1",
                "refresh-1",
                &callback,
            ] {
                assert!(!public.contains(secret));
            }
            let process::Fixture {
                directory,
                node,
                controller,
                client,
                session,
                ..
            } = fixture;
            drop(client);
            node.shutdown().await.unwrap();
            let database = rusqlite::Connection::open_with_flags(
                directory.path().join("node/storage/node.sqlite3"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let body: Vec<u8> = database
                .query_row(
                    "SELECT body FROM requests WHERE id=?1",
                    [complete.id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(body.starts_with(b"credential-v1:"));
            assert!(!String::from_utf8(body).unwrap().contains(&callback));
            drop(database);
            let node = Node::start(directory.path().join("node")).await.unwrap();
            let client = Client::new(if remote {
                controller.handle().remote(node.link().address())
            } else {
                node.local()
            });
            assert_eq!(client.execute(complete).await.unwrap(), receipt);
            assert_eq!(
                client.execute(begin_request).await.unwrap(),
                Output::McpLogin(attempt.clone())
            );
            let missing = match client.subscribe_mcp_login(attempt.id).await {
                Ok(mut stream) => stream.next().await.unwrap_err(),
                Err(error) => error,
            };
            assert_eq!(missing.code, ErrorCode::NotFound);
            assert_eq!(
                client.execute(request).await.unwrap(),
                Output::QueuedTurn(turn)
            );
            assert_eq!(history(&client, session.id).await, page);
            let Output::McpAuthorization(restored) = execute(
                &client,
                Command::ReadMcpAuthorization {
                    package: connected.clone(),
                    server: "input".into(),
                },
            )
            .await
            else {
                panic!("authorization status expected")
            };
            assert!(restored.configured);
            execute(
                &client,
                Command::RevokeMcpAuthorization {
                    package: connected,
                    server: "input".into(),
                },
            )
            .await;
            assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
            assert_eq!(input.calls.load(Ordering::SeqCst), 1);
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn discards_stale_grants() {
    for remote in [false, true] {
        let (fixture, _, _, issuer, package) = setup(remote, false).await;
        let (_, attempt, url) = begin(&fixture.client, package.clone()).await;
        let Output::McpAuthorization(changed) = execute(
            &fixture.client,
            Command::RevokeMcpAuthorization {
                package,
                server: "input".into(),
            },
        )
        .await
        else {
            panic!("authorization status expected")
        };
        execute(
            &fixture.client,
            Command::CompleteMcpLogin {
                attempt: attempt.id,
                callback: Secret::new(issuer::Server::callback(&url).await),
            },
        )
        .await;
        let failed = state(&fixture.client, &attempt, |state| {
            matches!(state, State::Failed(_))
        })
        .await;
        assert!(
            matches!(failed, State::Failed(error) if error.code == ErrorCode::RevisionConflict)
        );
        assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 1);
        let Output::McpAuthorization(status) = execute(
            &fixture.client,
            Command::ReadMcpAuthorization {
                package: changed.package,
                server: "input".into(),
            },
        )
        .await
        else {
            panic!("authorization status expected")
        };
        assert!(!status.configured);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_invalid_callbacks() {
    for remote in [false, true] {
        for cancel in [false, true] {
            let (fixture, _, _, issuer, package) = setup(remote, false).await;
            let (request, attempt, url) = begin(&fixture.client, package.clone()).await;
            let mut callback = reqwest::Url::parse(&issuer::Server::callback(&url).await).unwrap();
            callback
                .query_pairs_mut()
                .append_pair("state", "wrong-state");
            if cancel {
                execute(
                    &fixture.client,
                    Command::CancelMcpLogin {
                        attempt: attempt.id,
                    },
                )
                .await;
                assert_eq!(
                    state(&fixture.client, &attempt, |s| matches!(s, State::Cancelled)).await,
                    State::Cancelled
                );
                assert_eq!(
                    fixture
                        .client
                        .execute(fixture.client.prepare(Command::CompleteMcpLogin {
                            attempt: attempt.id,
                            callback: Secret::new(callback.to_string())
                        }))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::Conflict
                );
            } else {
                // The callback route is bound to the original controller's listener.
                callback.set_path("/different-callback");
                execute(
                    &fixture.client,
                    Command::CompleteMcpLogin {
                        attempt: attempt.id,
                        callback: Secret::new(callback.to_string()),
                    },
                )
                .await;
                assert!(matches!(
                    state(&fixture.client, &attempt, |s| matches!(s, State::Failed(_))).await,
                    State::Failed(_)
                ));
            }
            assert_eq!(issuer.exchanges.load(Ordering::SeqCst), 0);
            assert_eq!(
                fixture.client.execute(request).await.unwrap(),
                Output::McpLogin(attempt)
            );
            let Output::McpAuthorization(status) = execute(
                &fixture.client,
                Command::ReadMcpAuthorization {
                    package,
                    server: "input".into(),
                },
            )
            .await
            else {
                panic!("status expected")
            };
            assert!(!status.configured);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
