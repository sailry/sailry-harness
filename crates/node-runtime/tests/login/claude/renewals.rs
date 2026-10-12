use super::*;

fn captured(fixture: &Fixture, turn: &QueuedTurn) -> Provider {
    let db = rusqlite::Connection::open_with_flags(
        fixture.node.profile().join("storage/node.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let body: Vec<u8> = db
        .query_row(
            "SELECT provider FROM agent_runs WHERE turn=?1",
            [turn.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn rotated(fixture: &Fixture, provider: &Provider) {
    let private = grant(fixture, provider);
    assert!(private["access"] == REFRESHED && private["refresh"] == ROTATED);
    assert_eq!(private["renewal_pending"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restores_rotated_tokens_and_tool_continuations() {
    for remote in [false, true] {
        let exchanges = Arc::new(AtomicUsize::new(0));
        let count = exchanges.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/v1/oauth/token" {
                let body: Value = serde_json::from_str(&request.body).unwrap();
                let fresh = body["grant_type"] == "refresh_token";
                if fresh {
                    assert_eq!(count.fetch_add(1, Ordering::SeqCst), 0);
                }
                Reply::Json(server::tokens(request, fresh, 3600))
            } else if request.path == "/v1/messages" {
                server::generation(request, REFRESHED, true)
            } else {
                server::standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = connected(&fixture).await;
        let session = super::super::execution::session(&fixture, &provider).await;
        std::fs::write(fixture.node.profile().join("instructions.md"), INSTRUCTION).unwrap();
        expire(&fixture, &provider);
        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;
        let (request, turn) = super::super::execution::submit(&fixture.client, &session).await;
        let page =
            super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
        redacted(&page);
        assert_eq!(answer(&page, turn.id), ANSWER);
        assert!(
            serde_json::to_string(&page)
                .unwrap()
                .contains("Isolated tool content")
        );
        let usages = page
            .entries
            .iter()
            .filter(|entry| entry.turn == turn.id)
            .filter_map(|entry| entry.usage.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(usages.len(), 2);
        for usage in usages {
            assert_eq!(
                (
                    usage.input,
                    usage.output,
                    usage.cached_input,
                    usage.reasoning
                ),
                (23, 2, 4, 0)
            );
        }
        assert_eq!(
            captured(&fixture, &turn).oauth,
            Options::defaults(Authentication::ClaudeOAuth)
        );
        assert_eq!(saved(&fixture.client, provider.id).await, provider);
        rotated(&fixture, &provider);
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;
        rotated(&fixture, &provider);
        let (_, next) = super::super::execution::submit(&fixture.client, &session).await;
        let page =
            super::super::execution::finished(&fixture.client, &next, Status::Completed).await;
        redacted(&page);
        assert_eq!(answer(&page, next.id), ANSWER);
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == "/v1/messages")
                    .count(),
                4
            );
        }
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn finishes_rotation_after_observer_cancellation() {
    for remote in [false, true] {
        let release = Arc::new(tokio::sync::Notify::new());
        let gate = release.clone();
        let exchanges = Arc::new(AtomicUsize::new(0));
        let count = exchanges.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/v1/oauth/token" {
                let body: Value = serde_json::from_str(&request.body).unwrap();
                let fresh = body["grant_type"] == "refresh_token";
                let tokens = server::tokens(request, fresh, 3600);
                if fresh {
                    assert_eq!(count.fetch_add(1, Ordering::SeqCst), 0);
                    Reply::Delayed(gate.clone(), tokens)
                } else {
                    Reply::Json(tokens)
                }
            } else if request.path == "/v1/messages" {
                server::generation(request, REFRESHED, true)
            } else {
                server::standard(request)
            }
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = connected(&fixture).await;
        let first = super::super::execution::session(&fixture, &provider).await;
        let second = super::super::execution::session(&fixture, &provider).await;
        std::fs::write(fixture.node.profile().join("instructions.md"), INSTRUCTION).unwrap();
        expire(&fixture, &provider);
        let (_, stopped) = super::super::execution::submit(&fixture.client, &first).await;
        server.wait_requests(4).await;
        let (_, waiting) = super::super::execution::submit(&fixture.client, &second).await;
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StopTurn { turn: stopped.id }),
            )
            .await
            .unwrap();
        let page =
            super::super::execution::finished(&fixture.client, &stopped, Status::Cancelled).await;
        redacted(&page);
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        release.notify_one();
        let page =
            super::super::execution::finished(&fixture.client, &waiting, Status::Completed).await;
        redacted(&page);
        assert_eq!(answer(&page, waiting.id), ANSWER);
        rotated(&fixture, &provider);
        let fixture = super::super::execution::renewals::restart(fixture, &server, remote).await;
        rotated(&fixture, &provider);
        let (_, next) = super::super::execution::submit(&fixture.client, &first).await;
        let page =
            super::super::execution::finished(&fixture.client, &next, Status::Completed).await;
        redacted(&page);
        assert_eq!(answer(&page, next.id), ANSWER);
        assert_eq!(exchanges.load(Ordering::SeqCst), 1);
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.path == "/v1/messages")
                    .count(),
                4
            );
        }
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retains_uncertain_refresh_without_replay() {
    for remote in [false, true] {
        for mode in ["denied", "shutdown"] {
            let exchanges = Arc::new(AtomicUsize::new(0));
            let count = exchanges.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if request.path == "/v1/oauth/token" {
                    let body: Value = serde_json::from_str(&request.body).unwrap();
                    if body["grant_type"] == "refresh_token" {
                        assert_eq!(count.fetch_add(1, Ordering::SeqCst), 0);
                        let _ = server::tokens(request, true, 3600);
                        if mode == "shutdown" { Reply::Hold }
                        else { Reply::Raw(format!("HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ROTATED}", ROTATED.len())) }
                    } else { Reply::Json(server::tokens(request, false, 3600)) }
                } else { server::standard(request) }
            }).await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = connected(&fixture).await;
            let session = super::super::execution::session(&fixture, &provider).await;
            expire(&fixture, &provider);
            let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
            server.wait_requests(4).await;
            if mode == "denied" {
                let page =
                    super::super::execution::finished(&fixture.client, &turn, Status::Failed).await;
                redacted(&page);
            }
            assert_eq!(grant(&fixture, &provider)["renewal_pending"], true);
            let fixture =
                super::super::execution::renewals::restart(fixture, &server, remote).await;
            if mode == "shutdown" {
                server.wait_closed(1).await;
            }
            let (_, next) = super::super::execution::submit(&fixture.client, &session).await;
            let page =
                super::super::execution::finished(&fixture.client, &next, Status::Failed).await;
            redacted(&page);
            assert_eq!(
                page.runs
                    .iter()
                    .find(|run| run.turn == next.id)
                    .unwrap()
                    .error
                    .as_ref()
                    .unwrap()
                    .code,
                ErrorCode::OutcomeUnknown
            );
            assert_eq!(exchanges.load(Ordering::SeqCst), 1);
            assert_eq!(server.requests.lock().unwrap().len(), 4);
            assert_eq!(saved(&fixture.client, provider.id).await, provider);
            fixture.close().await;
        }
    }
}
