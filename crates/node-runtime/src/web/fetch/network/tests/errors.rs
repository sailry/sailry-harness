use super::*;

#[tokio::test]
async fn classifies_timeout_without_private_details() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(1))
        .build()
        .unwrap();
    let error = client
        .get(format!(
            "http://{}/private-path?token=private-token",
            listener.local_addr().unwrap()
        ))
        .header("Authorization", "Bearer private-header")
        .send()
        .await
        .unwrap_err();
    let fault = request_error(error);
    assert_eq!(fault.code, ErrorCode::Unavailable);
    assert_eq!(fault.message, "web request timed out");
}

#[tokio::test]
async fn classifies_connection_failure_without_private_details() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let owner = configured(server::HOST, address, None);
    let fault = fetch(
        &owner,
        Url::parse(&format!(
            "http://{}:{}/private-path?token=private-token",
            server::HOST,
            address.port()
        ))
        .unwrap(),
    )
    .await
    .err()
    .expect("connection must fail");
    assert_eq!(fault.code, ErrorCode::Unavailable);
    assert_eq!(fault.message, "web connection failed");
}

#[tokio::test]
async fn preserves_http_failure_status_and_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        assert!(stream.read(&mut buffer).await.unwrap() > 0);
        let body = r#"{"message":"Authentication required"}"#;
        stream
            .write_all(
                format!("HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes(),
            )
            .await
            .unwrap();
    });
    let owner = configured(server::HOST, address, None);
    let page = fetch(
        &owner,
        Url::parse(&format!(
            "http://{}:{}/denied",
            server::HOST,
            address.port()
        ))
        .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(page.status, 403);
    assert_eq!(page.content, r#"{"message":"Authentication required"}"#);
    peer.await.unwrap();
}
