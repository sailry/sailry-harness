use super::*;
use sailry_protocol::plugin::connector::{Input, LoginState};
use sailry_protocol::{Command, Output, Update};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    endpoint: String,
    posts: Arc<Mutex<Vec<Value>>>,
    gets: Arc<AtomicUsize>,
    refreshes: Arc<AtomicUsize>,
    expected: Arc<Mutex<String>>,
    reuse: Arc<AtomicBool>,
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
            reuse: Arc::default(),
            stop: CancellationToken::new(),
        };
        let (posts, gets, refreshes, expected, reuse, stop) = (
            result.posts.clone(),
            result.gets.clone(),
            result.refreshes.clone(),
            result.expected.clone(),
            result.reuse.clone(),
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
                    json!({"issuer":origin,"authorization_endpoint":format!("{origin}/authorize"),"token_endpoint":format!("{origin}/token"),"registration_endpoint":format!("{origin}/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["none"]}).to_string()
                } else if path == "/register" {
                    let mut registration: Value =
                        serde_json::from_slice(&bytes[end..end + size]).unwrap();
                    registration["client_id"] = json!("fixture-client");
                    status = 201;
                    registration.to_string()
                } else if path == "/token" {
                    let revision = refreshes.fetch_add(1, Ordering::SeqCst) + 1;
                    let token = format!(
                        "rotated-{}",
                        revision - usize::from(reuse.load(Ordering::SeqCst))
                    );
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
                        if head.starts_with("POST ") {
                            extra = format!(
                                "WWW-Authenticate: Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\"\r\n"
                            );
                        }
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
    node.set_connector_fixture("https://mcp.notion.com", &server.endpoint);
    let controller = sailry_client::Client::new(node.local());
    let Output::Plugin(info) = controller
        .execute(controller.prepare(Command::InstallBundledPlugin {
            name: "notion".into(),
            expected_revision: 0,
        }))
        .await
        .unwrap()
    else {
        panic!("plugin expected")
    };
    let redirect = "http://127.0.0.1:12345/auth/callback";
    let Output::ConnectorLogin(attempt) = controller
        .execute(controller.prepare(Command::StartConnectorLogin {
            package: info.summary.reference(),
            connector: "notion".into(),
            connection: None,
            expected_revision: None,
            method: Kind::McpOauth,
            input: Input {
                redirect: Some(redirect.into()),
                ..Default::default()
            },
        }))
        .await
        .unwrap()
    else {
        panic!("login attempt expected")
    };
    let mut updates = controller
        .subscribe_connector_login(attempt.id)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Update::ConnectorLogin(update) = updates.next().await.unwrap() else {
                panic!("login update expected")
            };
            match update.state {
                LoginState::Browser { url, .. } => {
                    let url = url::Url::parse(&url).unwrap();
                    let state = url
                        .query_pairs()
                        .find(|(key, _)| key == "state")
                        .unwrap()
                        .1
                        .into_owned();
                    let mut callback = url::Url::parse(redirect).unwrap();
                    callback
                        .query_pairs_mut()
                        .append_pair("code", "fixture-code")
                        .append_pair("state", &state);
                    controller
                        .execute(controller.prepare(Command::SubmitConnectorInput {
                            attempt: attempt.id,
                            input: Secret::new(callback.into()),
                        }))
                        .await
                        .unwrap();
                }
                LoginState::Connected(_) => break,
                state => assert!(state.active(), "{state:?}"),
            }
        }
    })
    .await
    .unwrap();
    let target = node
        .ingress
        .connector_target(attempt.connection, 1, false)
        .await
        .unwrap();
    let injection = target.declaration.inject[0].clone();
    let oauth = node
        .ingress
        .plugins
        .authorizations
        .client(
            &node.ingress,
            crate::plugins::authorization::Target {
                plugin: "notion".into(),
                server: "notion".into(),
                endpoint: "https://mcp.notion.com/mcp".into(),
                id: target.id,
                revision: target.revision,
                checking: false,
            },
            None,
        )
        .await
        .unwrap();
    let mut authorization = Authorization::new(
        node.ingress.clone(),
        vec![(target, injection)],
        CancellationToken::new(),
    );
    authorization.oauth = Some(oauth);
    server.refreshes.store(0, Ordering::SeqCst);
    *server.expected.lock().unwrap() = "rejected".into();
    let client = authorization.client(inner, 65536);
    let message:ClientJsonRpcMessage=serde_json::from_value(json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"write","arguments":{"value":"once"}}})).unwrap();
    assert!(
        client
            .post_message(
                "https://mcp.notion.com/mcp".into(),
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
    let stream = client
        .get_stream(
            "https://mcp.notion.com/mcp".into(),
            None,
            None,
            None,
            Headers::new(),
        )
        .await
        .unwrap();
    drop(stream);
    assert_eq!(server.refreshes.load(Ordering::SeqCst), 1);
    let baseline = server.gets.load(Ordering::SeqCst);
    *server.expected.lock().unwrap() = "expired-again".into();
    let stream = client
        .get_stream(
            "https://mcp.notion.com/mcp".into(),
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
    server.reuse.store(true, Ordering::SeqCst);
    *server.expected.lock().unwrap() = "rejected-again".into();
    let baseline = server.gets.load(Ordering::SeqCst);
    let stream = client
        .get_stream(
            "https://mcp.notion.com/mcp".into(),
            None,
            None,
            None,
            Headers::new(),
        )
        .await
        .unwrap();
    drop(stream);
    assert_eq!(server.gets.load(Ordering::SeqCst) - baseline, 2);
    assert_eq!(server.refreshes.load(Ordering::SeqCst), 3);
    assert_eq!(server.posts.lock().unwrap().len(), 1);
    node.shutdown().await.unwrap();
}
