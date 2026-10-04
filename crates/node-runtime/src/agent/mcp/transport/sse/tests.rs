use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

async fn request(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 64 * 1024);
        if bytes.ends_with(b"\r\n\r\n") {
            return String::from_utf8(bytes).unwrap();
        }
    }
}

async fn source(endpoint: &str, messages: String) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/sse", listener.local_addr().unwrap());
    let endpoint = endpoint.replace("{authority}", &listener.local_addr().unwrap().to_string());
    let peer = tokio::spawn(async move {
        let (mut events, _) = listener.accept().await.unwrap();
        let get = request(&mut events).await;
        assert!(get.starts_with("GET /sse "));
        assert!(get.contains("authorization: Bearer fixture\r\n"));
        events.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\nevent: endpoint\ndata: {endpoint}\n\n{messages}").as_bytes()).await.ok();
        // Keep the GET alive while receiving the independent client POST.
        let (mut post, _) = listener.accept().await.unwrap();
        let head = request(&mut post).await;
        assert!(head.starts_with("POST /messages?session=fixture "));
        assert!(head.contains("authorization: Bearer fixture\r\n"));
        assert!(!head.contains("mcp-session-id:") && !head.contains("last-event-id:"));
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .unwrap()
            .parse()
            .unwrap();
        let mut body = vec![0; length];
        post.read_exact(&mut body).await.unwrap();
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["method"], "ping");
        post.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut byte = [0];
        assert_eq!(events.read(&mut byte).await.unwrap(), 0);
    });
    (url, peer)
}

async fn connect(url: &str) -> io::Result<Connection> {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    Connection::open(
        client,
        url,
        HashMap::from([(
            reqwest::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer fixture"),
        )]),
    )
    .await
}

#[tokio::test]
async fn uses_advertised_path_with_headers() {
    let (url, peer) = source(
        "/messages?session=fixture",
        "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n\n".into(),
    )
    .await;
    let mut connection = connect(&url).await.unwrap();
    connection
        .send(serde_json::from_value(json!({"jsonrpc":"2.0","id":7,"method":"ping"})).unwrap())
        .await
        .unwrap();
    let message = connection.receive().await.unwrap();
    assert_eq!(serde_json::to_value(message).unwrap()["id"], 7);
    connection.close().await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), peer)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn rejects_untrusted_endpoints() {
    for endpoint in [
        "https://example.invalid/messages",
        "//127.0.0.1:1/messages",
        "http://user:password@{authority}/messages",
        "/messages#fragment",
    ] {
        let (url, peer) = source(endpoint, String::new()).await;
        assert!(
            connect(&url).await.is_err(),
            "endpoint must be rejected: {endpoint}"
        );
        peer.abort();
        assert!(peer.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn closes_invalid_streams() {
    for message in [
        "event: message\ndata: invalid JSON\n\n".to_owned(),
        format!("event: message\ndata: {}", "x".repeat(limits::MAX_MESSAGE)),
        "event: endpoint\ndata: /replacement\n\n".to_owned(),
    ] {
        let (url, peer) = source("/messages?session=fixture", message).await;
        let mut connection = connect(&url).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), connection.receive())
                .await
                .unwrap()
                .is_none()
        );
        assert!(connection.stop.is_cancelled());
        peer.abort();
        assert!(peer.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn cancels_pending_post() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/sse", listener.local_addr().unwrap());
    let (posted, received) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut events, _) = listener.accept().await.unwrap();
        request(&mut events).await;
        events.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\nevent: endpoint\ndata: /messages\n\n").await.unwrap();
        let (mut post, _) = listener.accept().await.unwrap();
        request(&mut post).await;
        posted.send(()).unwrap();
        // Retain the POST without a response until the client closes its GET.
        let mut byte = [0];
        assert_eq!(events.read(&mut byte).await.unwrap(), 0);
        drop(post);
    });
    let mut connection = connect(&url).await.unwrap();
    let send = connection
        .send(serde_json::from_value(json!({"jsonrpc":"2.0","id":7,"method":"ping"})).unwrap());
    let send = tokio::spawn(send);
    tokio::time::timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    connection.close().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), send)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(2), peer)
        .await
        .unwrap()
        .unwrap();
}
