use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adopts_required_version_before_generation() {
    for (remote, required) in [(false, "2.1.990"), (true, "2.1.991")] {
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/v1/oauth/token" {
                return Reply::Json(server::tokens(request, false, 3600));
            }
            if request.path.starts_with("/v1/models") {
                return server::catalog(request, ACCESS, &request.headers["user-agent"]);
            }
            assert_eq!(request.path, "/v1/messages?beta=true");
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                return rejection(required);
            }
            assert_eq!(
                request.headers["user-agent"],
                format!("claude-cli/{required} (external, cli)")
            );
            let body: Value = serde_json::from_str(&request.body).unwrap();
            assert!(
                body["system"][0]["text"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!(
                        "x-anthropic-billing-header: cc_version={required}."
                    ))
            );
            server::generation_version(request, ACCESS, false, required)
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = connected(&fixture).await;
        let session = super::super::execution::session(&fixture, &provider).await;
        std::fs::write(fixture.node.profile().join("instructions.md"), INSTRUCTION).unwrap();
        let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
        let page =
            super::super::execution::finished(&fixture.client, &turn, Status::Completed).await;
        redacted(&page);
        assert_eq!(answer(&page, turn.id), ANSWER);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == turn.id && entry.usage.is_some())
                .count(),
            1
        );
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stops_after_one_version_retry() {
    for (remote, version) in [(false, 990), (true, 992)] {
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/v1/oauth/token" {
                return Reply::Json(server::tokens(request, false, 3600));
            }
            if request.path.starts_with("/v1/models") {
                return server::catalog(request, ACCESS, &request.headers["user-agent"]);
            }
            assert_eq!(request.path, "/v1/messages?beta=true");
            let attempt = count.fetch_add(1, Ordering::SeqCst);
            assert!(attempt < 2, "version negotiation must not loop");
            rejection(&format!("2.1.{}", version + attempt))
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = connected(&fixture).await;
        let session = super::super::execution::session(&fixture, &provider).await;
        let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
        let page = super::super::execution::finished(&fixture.client, &turn, Status::Failed).await;
        redacted(&page);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(
            page.entries
                .iter()
                .all(|entry| entry.turn != turn.id || entry.usage.is_none())
        );
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancels_pending_version_retry() {
    for (remote, required) in [(false, "2.1.990"), (true, "2.1.991")] {
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/v1/oauth/token" {
                return Reply::Json(server::tokens(request, false, 3600));
            }
            if request.path.starts_with("/v1/models") {
                return server::catalog(request, ACCESS, &request.headers["user-agent"]);
            }
            assert_eq!(request.path, "/v1/messages?beta=true");
            match count.fetch_add(1, Ordering::SeqCst) {
                0 => rejection(required),
                1 => {
                    assert_eq!(
                        request.headers["user-agent"],
                        format!("claude-cli/{required} (external, cli)")
                    );
                    Reply::Hold
                }
                _ => panic!("cancelled generation must not replay"),
            }
        })
        .await;
        let fixture = Fixture::empty(remote, &server).await;
        let provider = connected(&fixture).await;
        let session = super::super::execution::session(&fixture, &provider).await;
        let (_, turn) = super::super::execution::submit(&fixture.client, &session).await;
        // Token exchange and two catalog pages precede the two inference attempts.
        server.wait_requests(5).await;
        fixture
            .client
            .execute(fixture.client.prepare(Command::StopTurn { turn: turn.id }))
            .await
            .unwrap();
        let page =
            super::super::execution::finished(&fixture.client, &turn, Status::Cancelled).await;
        server.wait_closed(1).await;
        redacted(&page);
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(
            page.entries
                .iter()
                .all(|entry| entry.turn != turn.id || entry.usage.is_none())
        );
        fixture.close().await;
    }
}

fn rejection(required: &str) -> Reply {
    let body = json!({"error":{"type":"invalid_request_error","message":format!("claude_code_version_too_old: version {required} or newer is required")}}).to_string();
    Reply::Raw(format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ))
}
