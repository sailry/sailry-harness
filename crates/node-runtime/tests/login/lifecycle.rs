use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preserves_revocation_and_reconnects() {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let ready = release.clone();
        let exchanges = std::sync::atomic::AtomicUsize::new(0);
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/copilot_internal/v2/token"
                && exchanges.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1
            {
                Reply::Delayed(
                    ready.clone(),
                    json!({"token":ACCESS,"expires_at":future_expiry()}),
                )
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::Copilot).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
        let Output::Providers(providers) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let provider = &providers[0];
        let reference = provider.credential.clone().unwrap();
        let (_, attempt) = begin(&fixture.client, provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        server.wait_requests(6).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::RevokeCredential {
                id: reference.id,
                expected_revision: 1,
            }))
            .await
            .unwrap();
        release.notify_one();
        let login::State::Failed(error) = terminal(&mut stream).await.state else {
            panic!("revocation conflict expected")
        };
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        let Output::Credentials(credentials) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCredentials))
            .await
            .unwrap()
        else {
            panic!("credentials expected")
        };
        assert!(credentials[0].revoked);
        let (_, attempt) = begin(&fixture.client, provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
        let Output::Credentials(credentials) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListCredentials))
            .await
            .unwrap()
        else {
            panic!("credentials expected")
        };
        assert_eq!(credentials.len(), 1);
        assert_eq!(credentials[0].id, reference.id);
        assert_eq!(credentials[0].revision, 3);
        assert!(!credentials[0].revoked);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::PutCredential {
                    id: reference.id,
                    provider: provider.id,
                    expected_revision: 3,
                    secret: Secret::new("isolated-api-key".into()),
                    expires_at_ms: None
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rolls_back_failed_save() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::ChatGpt).await;
        let db = rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
            .unwrap();
        let before: i64 = db
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap();
        db.execute_batch("CREATE TRIGGER reject_provider_update BEFORE UPDATE ON providers BEGIN SELECT RAISE(ABORT,'isolated provider failure'); END;").unwrap();
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert!(matches!(
            terminal(&mut stream).await.state,
            login::State::Failed(_)
        ));
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap(),
            Output::Credentials(vec![])
        );
        assert_eq!(
            db.query_row::<i64, _, _>("SELECT count(*) FROM events", [], |row| row.get(0))
                .unwrap(),
            before
        );
        let mut query = db
            .prepare("SELECT CAST(body AS TEXT),coalesce(result,'') FROM requests")
            .unwrap();
        for row in query
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
        {
            let (body, result) = row.unwrap();
            no_secrets(&body);
            no_secrets(&result);
            assert!(!body.contains("ABCD-1234"));
            assert!(!result.contains("ABCD-1234"));
        }
        drop(query);
        drop(db);
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancels_pending_http() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |request| {
            if request.path.ends_with("deviceauth/token") {
                Reply::Hold
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::ChatGpt).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        server.wait_requests(2).await;
        let cancel = fixture.client.prepare(Command::CancelProviderLogin {
            attempt: attempt.id,
        });
        let output = fixture.client.execute(cancel.clone()).await.unwrap();
        assert_eq!(terminal(&mut stream).await.state, login::State::Cancelled);
        server.wait_closed(1).await;
        assert_eq!(fixture.client.execute(cancel).await.unwrap(), output);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap(),
            Output::Credentials(vec![])
        );
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ListProviders))
                .await
                .unwrap(),
            Output::Providers(vec![provider.clone()])
        );
        let (_, retry) = begin(&fixture.client, &provider).await;
        assert_ne!(retry.id, attempt.id);
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::CancelProviderLogin { attempt: retry.id }),
            )
            .await
            .unwrap();
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_conflicts_and_other_callers() {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let ready = release.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/oauth/token" {
                Reply::Delayed(
                    ready.clone(),
                    json!({"access_token":access_token(),"refresh_token":REFRESH}),
                )
            } else {
                standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let mut provider = provider(&fixture.client, Authentication::ChatGpt).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        let alternate = Link::controller(
            fixture.directory.path().join("alternate"),
            NetworkScope::default(),
        )
        .await
        .unwrap();
        let address = alternate
            .handle()
            .pair(fixture.node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let other = Client::new(alternate.handle().remote(address));
        assert_eq!(
            subscription_error(other.subscribe_login(attempt.id).await)
                .await
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            other
                .execute(other.prepare(Command::CancelProviderLogin {
                    attempt: attempt.id
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::BeginProviderLogin {
                    provider: provider.id,
                    expected_revision: provider.revision
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        server.wait_requests(3).await;
        provider.name = "Edited during login".into();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        release.notify_one();
        let login::State::Failed(failure) = terminal(&mut stream).await.state else {
            panic!("revision failure expected")
        };
        assert_eq!(failure.code, ErrorCode::RevisionConflict);
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::ListCredentials))
                .await
                .unwrap(),
            Output::Credentials(vec![])
        );
        alternate.close().await.unwrap();
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shuts_down_and_reopens() {
    let server = Server::start_with_request(ModelApi::Anthropic, |_| Reply::Hold).await;
    let fixture = Fixture::new(true, &server).await;
    let provider = provider(&fixture.client, Authentication::Copilot).await;
    begin(&fixture.client, &provider).await;
    server.wait_requests(1).await;
    let path = fixture.directory.path().join("node");
    tokio::time::timeout(Duration::from_secs(5), fixture.node.shutdown())
        .await
        .unwrap()
        .unwrap();
    server.wait_closed(1).await;
    let node = Node::start_with_authorization(&path, &server.endpoint)
        .await
        .unwrap();
    let client = Client::new(node.local());
    assert_eq!(
        client
            .execute(client.prepare(Command::ListCredentials))
            .await
            .unwrap(),
        Output::Credentials(vec![])
    );
    fixture.controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bounds_attempts_without_observers() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, |_| Reply::Hold).await;
        let fixture = Fixture::new(remote, &server).await;
        let mut attempts = Vec::new();
        for _ in 0..4 {
            let provider = provider(&fixture.client, Authentication::Copilot).await;
            let (_, attempt) = begin(&fixture.client, &provider).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            assert_eq!(next(&mut stream).await.state, login::State::Starting);
            drop(stream);
            attempts.push(attempt);
        }
        server.wait_requests(4).await;
        let provider = provider(&fixture.client, Authentication::Copilot).await;
        assert_eq!(
            fixture
                .client
                .execute(fixture.client.prepare(Command::BeginProviderLogin {
                    provider: provider.id,
                    expected_revision: provider.revision
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        for attempt in attempts {
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            assert_eq!(next(&mut stream).await.state, login::State::Starting);
            fixture
                .client
                .execute(fixture.client.prepare(Command::CancelProviderLogin {
                    attempt: attempt.id,
                }))
                .await
                .unwrap();
            assert_eq!(terminal(&mut stream).await.state, login::State::Cancelled);
        }
        server.wait_closed(4).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::CancelProviderLogin {
                attempt: attempt.id,
            }))
            .await
            .unwrap();
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn requires_recorded_receipt() {
    for remote in [false, true] {
        let server = Server::start_with_request(ModelApi::Anthropic, standard).await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::ChatGpt).await;
        let db = rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
            .unwrap();
        db.execute_batch("CREATE TRIGGER reject_login_receipt BEFORE UPDATE ON requests BEGIN SELECT RAISE(ABORT,'isolated receipt failure'); END;").unwrap();
        let request = fixture.client.prepare(Command::BeginProviderLogin {
            provider: provider.id,
            expected_revision: provider.revision,
        });
        assert_eq!(
            fixture
                .client
                .execute(request.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
        assert!(server.requests.lock().unwrap().is_empty());
        db.execute_batch("DROP TRIGGER reject_login_receipt;")
            .unwrap();
        assert_eq!(
            fixture.client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(server.requests.lock().unwrap().is_empty());
        drop(db);
        fixture.close().await;
    }
}
