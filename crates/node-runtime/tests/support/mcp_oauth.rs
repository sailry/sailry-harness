//! Isolated OAuth issuer and authenticated proxy for the shared MCP peer.
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub struct Server {
    pub endpoint: String,
    pub exchanges: Arc<AtomicUsize>,
    pub refreshes: Arc<AtomicUsize>,
    token: Arc<Mutex<String>>,
    stop: CancellationToken,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Server {
    pub async fn start(upstream: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let endpoint = format!("{origin}/mcp");
        let target = url::Url::parse(upstream).unwrap();
        let target = format!("{}:{}", target.host_str().unwrap(), target.port().unwrap());
        let exchanges = Arc::new(AtomicUsize::new(0));
        let refreshes = Arc::new(AtomicUsize::new(0));
        let token = Arc::new(Mutex::new("access-initial".to_owned()));
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let issuer = Arc::new(Issuer {
            origin,
            exchanges: exchanges.clone(),
            refreshes: refreshes.clone(),
            token: token.clone(),
            refresh: Mutex::new("refresh-initial".into()),
            callback: Mutex::new(None),
        });
        tokio::spawn(async move {
            let mut work = tokio::task::JoinSet::<io::Result<()>>::new();
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => break,
                    Some(result) = work.join_next(), if !work.is_empty() => { result.unwrap().unwrap(); },
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let (issuer, target) = (issuer.clone(), target.clone());
                        work.spawn(async move { issuer.respond(stream, &target).await });
                    }
                }
            }
            work.shutdown().await;
        });
        Self {
            endpoint,
            exchanges,
            refreshes,
            token,
            stop,
        }
    }

    pub fn expire(&self) {
        *self.token.lock().unwrap() = "expired-access".into();
    }

    pub async fn callback(url: &str) -> String {
        visit(url)
            .await
            .lines()
            .find_map(|line| line.strip_prefix("Location: "))
            .unwrap()
            .to_owned()
    }
}

struct Issuer {
    origin: String,
    exchanges: Arc<AtomicUsize>,
    refreshes: Arc<AtomicUsize>,
    token: Arc<Mutex<String>>,
    refresh: Mutex<String>,
    callback: Mutex<Option<String>>,
}

pub async fn visit(value: &str) -> String {
    let url = url::Url::parse(value).unwrap();
    assert_eq!(url.scheme(), "http");
    let mut stream = TcpStream::connect((url.host_str().unwrap(), url.port().unwrap()))
        .await
        .unwrap();
    let path = format!(
        "{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    stream
        .write_all(
            format!(
                "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                url.host_str().unwrap()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_string(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    response
}

impl Issuer {
    async fn respond(&self, mut stream: TcpStream, target: &str) -> io::Result<()> {
        let mut bytes = Vec::new();
        let mut buffer = [0; 8192];
        let end = loop {
            let count = stream.read(&mut buffer).await?;
            if count == 0 {
                return Ok(());
            }
            bytes.extend_from_slice(&buffer[..count]);
            assert!(bytes.len() < 1024 * 1024);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let head = std::str::from_utf8(&bytes[..end]).unwrap();
        let target_path = head.split_whitespace().nth(1).unwrap().to_owned();
        let headers: BTreeMap<_, _> = head
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        let length = headers
            .get("content-length")
            .map(|v| v.parse::<usize>().unwrap())
            .unwrap_or(0);
        while bytes.len() < end + length {
            let count = stream.read(&mut buffer).await?;
            if count == 0 {
                return Ok(());
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
        let url = url::Url::parse(&format!("{}{target_path}", self.origin)).unwrap();
        let body = &bytes[end..end + length];
        let origin = &self.origin;
        let result = match url.path() {
            "/.well-known/oauth-protected-resource/mcp"
            | "/.well-known/oauth-protected-resource" => {
                json!({"resource":format!("{origin}/mcp"),"authorization_servers":[origin],"scopes_supported":["tools"]})
            }
            "/.well-known/oauth-authorization-server" | "/.well-known/openid-configuration" => {
                json!({"issuer":origin,"authorization_endpoint":format!("{origin}/authorize"),"token_endpoint":format!("{origin}/token"),"registration_endpoint":format!("{origin}/register"),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"],"scopes_supported":["tools"]})
            }
            "/register" => {
                let mut registration: Value = serde_json::from_slice(body).unwrap();
                registration["client_id"] = json!("fixture-client");
                return reply(&mut stream, 201, "", registration).await;
            }
            "/authorize" => {
                let params: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
                assert_eq!(params["code_challenge_method"], "S256");
                assert!(!params["code_challenge"].is_empty());
                let mut callback = url::Url::parse(&params["redirect_uri"]).unwrap();
                *self.callback.lock().unwrap() = Some(params["redirect_uri"].clone());
                callback
                    .query_pairs_mut()
                    .append_pair("code", "fixture-code")
                    .append_pair("state", &params["state"])
                    .append_pair("iss", origin);
                return reply(
                    &mut stream,
                    302,
                    &format!("Location: {callback}\r\n"),
                    Value::Null,
                )
                .await;
            }
            "/token" => {
                let mut form = url::Url::parse("http://fixture/").unwrap();
                form.set_query(Some(std::str::from_utf8(body).unwrap()));
                let params: BTreeMap<_, _> = form.query_pairs().into_owned().collect();
                if params["grant_type"] == "authorization_code" {
                    assert_eq!(params["code"], "fixture-code");
                    assert!(!params["code_verifier"].is_empty());
                    assert_eq!(
                        Some(&params["redirect_uri"]),
                        self.callback.lock().unwrap().as_ref()
                    );
                    self.exchanges.fetch_add(1, Ordering::SeqCst);
                } else {
                    assert_eq!(params["grant_type"], "refresh_token");
                    assert_eq!(params["refresh_token"], *self.refresh.lock().unwrap());
                    let revision = self.refreshes.fetch_add(1, Ordering::SeqCst) + 1;
                    *self.token.lock().unwrap() = format!("access-{revision}");
                    *self.refresh.lock().unwrap() = format!("refresh-{revision}");
                }
                json!({"access_token":*self.token.lock().unwrap(),"token_type":"Bearer","refresh_token":*self.refresh.lock().unwrap(),"expires_in":3600,"scope":"tools"})
            }
            "/mcp" | "/messages" => {
                let expected = format!("Bearer {}", self.token.lock().unwrap());
                if headers.get("authorization") != Some(&expected) {
                    return reply(&mut stream,401,&format!("WWW-Authenticate: Bearer resource_metadata=\"{origin}/.well-known/oauth-protected-resource/mcp\"\r\n"), json!({"error":"unauthorized"})).await;
                }
                let mut upstream = TcpStream::connect(target).await?;
                upstream.write_all(&bytes).await?;
                // Preserve the original streaming response and SSE connection lifecycle.
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
                return Ok(());
            }
            _ => return reply(&mut stream, 404, "", Value::Null).await,
        };
        reply(&mut stream, 200, "", result).await
    }
}

async fn reply(stream: &mut TcpStream, status: u16, headers: &str, value: Value) -> io::Result<()> {
    let body = value.to_string();
    stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}",body.len()).as_bytes()).await
}
