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
            "Return to Sailry".into(),
            CancellationToken::new(),
        ));
        let reply = send(
            &redirect,
            &format!("{path}?state=expected&code=isolated-code"),
        )
        .await;
        assert!(reply.starts_with("HTTP/1.1 200 OK"));
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
        "Done".into(),
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
        "Done".into(),
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
            "Done".into(),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(result.unwrap_err().code, ErrorCode::Unavailable);
}
