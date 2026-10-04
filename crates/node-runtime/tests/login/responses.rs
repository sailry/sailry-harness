use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn honors_vendor_pending_and_slow_down() {
    for remote in [false, true] {
        let polls = std::sync::atomic::AtomicUsize::new(0);
        let timestamps = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = timestamps.clone();
        let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
            if request.path == "/login/oauth/access_token" {
                recorded.lock().unwrap().push(std::time::Instant::now());
                match polls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) {
                    0 => return Reply::Json(json!({"error":"authorization_pending"})),
                    1 => return Reply::Json(json!({"error":"slow_down"})),
                    _ => {}
                }
            }
            standard(request)
        })
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let provider = provider(&fixture.client, Authentication::Copilot).await;
        let (_, attempt) = begin(&fixture.client, &provider).await;
        let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
        assert_eq!(terminal(&mut stream).await.state, login::State::Connected);
        let recorded = timestamps.lock().unwrap().clone();
        assert_eq!(recorded.len(), 3);
        assert!(recorded[1].duration_since(recorded[0]) >= Duration::from_secs(1));
        assert!(recorded[2].duration_since(recorded[1]) >= Duration::from_secs(6));
        fixture.close().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_unsafe_responses() {
    for remote in [false, true] {
        for case in ["redirect", "oversized", "routing", "denied", "verification"] {
            let sink = Server::start(ModelApi::Anthropic, |_| Reply::Hold).await;
            let destination = sink.endpoint.clone();
            let server = Server::start_with_request(ModelApi::Anthropic, move |request| {
                if case == "verification" && request.path == "/login/device/code" {
                    return Reply::Json(json!({"device_code":DEVICE,"user_code":"ABCD-1234","verification_uri":"https://attacker.invalid/login","interval":1}));
                }
                if request.path != "/copilot_internal/v2/token" { return standard(request); }
                match case {
                    "redirect" => Reply::Raw(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")),
                    "oversized" => Reply::Raw("HTTP/1.1 200 OK\r\nContent-Length: 65537\r\nConnection: close\r\n\r\n".into()),
                    "routing" => Reply::Json(json!({"token":ACCESS,"expires_at":future_expiry(),"endpoints":{"api":"https://api.githubcopilot.com.attacker.invalid"}})),
                    "denied" => Reply::Raw(format!("HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{GITHUB}", GITHUB.len())),
                    _ => unreachable!(),
                }
            }).await;
            let fixture = Fixture::new(remote, &server).await;
            let provider = provider(&fixture.client, Authentication::Copilot).await;
            let (_, attempt) = begin(&fixture.client, &provider).await;
            let mut stream = fixture.client.subscribe_login(attempt.id).await.unwrap();
            let result = terminal(&mut stream).await;
            assert!(matches!(result.state, login::State::Failed(_)));
            no_secrets(&result);
            assert!(sink.requests.lock().unwrap().is_empty());
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::ListCredentials))
                    .await
                    .unwrap(),
                Output::Credentials(vec![])
            );
            fixture.close().await;
        }
    }
}
