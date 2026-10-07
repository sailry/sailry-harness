use super::*;

async fn send(redirect: &str, target: &str) -> String {
    let url = Url::parse(redirect).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", url.port().unwrap()))
        .await
        .unwrap();
    stream
        .write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.unwrap();
    reply
}

#[tokio::test]
async fn receives_matching_state_on_each_path() {
    for path in ["/callback", "/auth/callback"] {
        let listener = Callback::bind(path).unwrap();
        let redirect = listener.redirect.clone();
        let task = tokio::spawn(listener.receive(
            "https://auth.example/authorize?state=expected".into(),
            async { Reply::Text("Return to Sailry".into()) },
            CancellationToken::new(),
        ));
        let reply = send(
            &redirect,
            &format!("{path}?state=expected&code=isolated-code"),
        )
        .await;
        assert!(reply.starts_with("HTTP/1.1 200 OK"));
        assert!(reply.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(reply.contains("Cache-Control: no-store"));
        assert!(reply.contains("Referrer-Policy: no-referrer"));
        assert!(reply.contains("X-Content-Type-Options: nosniff"));
        assert!(reply.ends_with("Return to Sailry"));
        let callback = task.await.unwrap().unwrap();
        assert_eq!(
            callback.expose(),
            format!("{redirect}?state=expected&code=isolated-code")
        );
    }
}

#[tokio::test]
async fn preserves_listener_after_invalid_state() {
    let listener = Callback::bind("/auth/callback").unwrap();
    let redirect = listener.redirect.clone();
    let task = tokio::spawn(listener.receive(
        "https://auth.example/authorize?state=expected".into(),
        async { Reply::Text("Done".into()) },
        CancellationToken::new(),
    ));
    for target in [
        "/callback?state=expected&code=wrong-path",
        "/auth/callback?state=wrong&code=wrong-state",
        "/auth/callback?state=expected&state=expected&code=duplicate",
        "http://example.test/auth/callback?state=expected&code=other-origin",
    ] {
        assert!(
            send(&redirect, target)
                .await
                .starts_with("HTTP/1.1 400 Bad Request")
        );
        assert!(!task.is_finished());
    }
    send(&redirect, "/auth/callback?state=expected&code=accepted").await;
    assert!(
        task.await
            .unwrap()
            .unwrap()
            .expose()
            .contains("code=accepted")
    );
}

#[tokio::test]
async fn cancellation_closes_listener() {
    let listener = Callback::bind("/auth/callback").unwrap();
    let stop = CancellationToken::new();
    let task = tokio::spawn(listener.receive(
        "https://auth.example/authorize?state=expected".into(),
        async { Reply::Text("Done".into()) },
        stop.clone(),
    ));
    stop.cancel();
    assert_eq!(task.await.unwrap().unwrap_err().code, ErrorCode::Cancelled);
}

#[tokio::test]
async fn rejects_missing_authorization_state() {
    let listener = Callback::bind("/auth/callback").unwrap();
    let result = listener
        .receive(
            "https://auth.example/authorize".into(),
            async { Reply::Text("Done".into()) },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(result.unwrap_err().code, ErrorCode::Unavailable);
}

#[tokio::test]
async fn returns_callback_before_deferred_reply() {
    let listener = Callback::bind("/auth/callback").unwrap();
    let redirect = listener.redirect.clone();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let callback = tokio::spawn(listener.receive(
        "https://auth.example/authorize?state=expected".into(),
        async move { receiver.await.unwrap() },
        CancellationToken::new(),
    ));
    let response = tokio::spawn(async move {
        send(&redirect, "/auth/callback?state=expected&code=accepted").await
    });
    let secret = tokio::time::timeout(Duration::from_secs(2), callback)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(secret.expose().contains("code=accepted"));
    assert!(!response.is_finished());
    sender
        .send(Reply::Html("<h1>Finished</h1>".into()))
        .ok()
        .unwrap();
    let response = response.await.unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("Content-Type: text/html; charset=utf-8"));
    assert!(response.ends_with("<h1>Finished</h1>"));
}
