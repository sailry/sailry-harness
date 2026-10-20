use super::*;
use rmcp::transport::auth::{
    AuthorizationManager, CredentialStore, InMemoryCredentialStore, StoredCredentials,
};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    endpoint: String,
    posts: Arc<Mutex<Vec<Value>>>,
    gets: Arc<AtomicUsize>,
    refreshes: Arc<AtomicUsize>,
    expected: Arc<Mutex<String>>,
    stop: CancellationToken,
}
impl Server {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let result = Self {
            endpoint: format!("{origin}/mcp"),
            posts: Arc::default(),
            gets: Arc::default(),
            refreshes: Arc::default(),
            expected: Arc::new(Mutex::new("rejected".into())),
            stop: CancellationToken::new(),
        };
        let (posts, gets, refreshes, expected, stop) = (
            result.posts.clone(),
            result.gets.clone(),
            result.refreshes.clone(),
            result.expected.clone(),
            result.stop.clone(),
        );
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = tokio::select! {_=stop.cancelled()=>break,result=listener.accept()=>result.unwrap()};
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let end = loop {
                    let count = stream.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        break 0;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                if end == 0 {
                    continue;
                }
                let head = std::str::from_utf8(&bytes[..end]).unwrap().to_owned();
                let size = head
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|value| value.parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < end + size {
                    let count = stream.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let path = head.split_whitespace().nth(1).unwrap();
                let mut status = 200;
                let mut content_type = "application/json";
                let mut extra = String::new();
                let body = if path.starts_with("/.well-known/oauth-protected-resource") {
                    json!({"resource":format!("{origin}/mcp"),"authorization_servers":[origin]})
                        .to_string()
                } else if path.starts_with("/.well-known/") {
                    json!({"issuer":origin,"authorization_endpoint":format!("{origin}/authorize"),"token_endpoint":format!("{origin}/token"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none"]}).to_string()
                } else if path == "/token" {
                    let revision = refreshes.fetch_add(1, Ordering::SeqCst) + 1;
                    let token = format!("rotated-{revision}");
                    *expected.lock().unwrap() = token.clone();
                    json!({"access_token":token,"refresh_token":format!("refresh-{revision}"),"expires_in":3600,"token_type":"Bearer"}).to_string()
                } else if path == "/mcp" {
                    if head.starts_with("POST ") {
                        posts
                            .lock()
                            .unwrap()
                            .push(serde_json::from_slice(&bytes[end..end + size]).unwrap());
                    } else if head.starts_with("GET ") {
                        gets.fetch_add(1, Ordering::SeqCst);
                    }
                    let authorization = head.lines().find_map(|line| {
                        line.split_once(':')
                            .filter(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                            .map(|(_, value)| value.trim())
                    });
                    if authorization
                        != Some(format!("Bearer {}", expected.lock().unwrap()).as_str())
                    {
                        status = 401;
                        extra = format!(
                            "WWW-Authenticate: Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\"\r\n"
                        );
                        json!({"error":"unauthorized"}).to_string()
                    } else {
                        content_type = "text/event-stream";
                        "event: message\ndata: {}\n\n".into()
                    }
                } else {
                    status = 404;
                    "{}".into()
                };
                let response = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        result
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[tokio::test]
async fn refreshes_without_replaying_posts() {
    let server = Server::start().await;
    let directory = tempfile::tempdir().unwrap();
    let node = crate::Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let inner = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
        .unwrap();
    let credentials = InMemoryCredentialStore::new();
    let token=serde_json::from_value(json!({"access_token":"old-access","refresh_token":"old-refresh","expires_in":3600,"token_type":"Bearer"})).unwrap();
    let received = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    credentials
        .save(StoredCredentials::new(
            "fixture-client".into(),
            Some(token),
            vec![],
            Some(received),
        ))
        .await
        .unwrap();
    let mut manager = AuthorizationManager::new(&server.endpoint).await.unwrap();
    manager.with_client(inner.clone()).unwrap();
    manager.set_credential_store(credentials);
    manager.resolve_metadata().await.unwrap();
    assert!(manager.initialize_from_store().await.unwrap());
    let mut authorization =
        Authorization::new(node.ingress.clone(), Vec::new(), CancellationToken::new());
    authorization.oauth = Some(AuthClient::new(inner.clone(), manager));
    let client = authorization.client(inner, 65536);
    let message:ClientJsonRpcMessage=serde_json::from_value(json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"write","arguments":{"value":"once"}}})).unwrap();
    assert!(
        client
            .post_message(
                server.endpoint.clone().into(),
                message,
                None,
                None,
                Headers::new()
            )
            .await
            .is_err()
    );
    assert_eq!(server.refreshes.load(Ordering::SeqCst), 1);
    {
        let posts = server.posts.lock().unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0]["id"], 7);
        assert_eq!(posts[0]["params"]["arguments"]["value"], "once");
    }
    let baseline = server.gets.load(Ordering::SeqCst);
    *server.expected.lock().unwrap() = "expired-again".into();
    let stream = client
        .get_stream(
            server.endpoint.clone().into(),
            None,
            None,
            None,
            Headers::new(),
        )
        .await
        .unwrap();
    drop(stream);
    assert_eq!(server.gets.load(Ordering::SeqCst) - baseline, 2);
    assert_eq!(server.refreshes.load(Ordering::SeqCst), 2);
    assert_eq!(server.posts.lock().unwrap().len(), 1);
    node.shutdown().await.unwrap();
}
