//! An owned token issuer proxies the finite application's admitted resource origin.
use sailry_link::CancellationToken;

use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub const SECRET: &str = "application-private-marker";
pub type Records = Arc<Mutex<Vec<(String, BTreeMap<String, String>, Value)>>>;

pub struct Server {
    pub origin: String,
    pub tokens: Arc<AtomicUsize>,
    pub records: Records,
    pub failed: Arc<AtomicBool>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start(upstream: Option<&str>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let tokens = Arc::new(AtomicUsize::new(0));
        let records: Records = Default::default();
        let failed = Arc::new(AtomicBool::new(false));
        let stop = CancellationToken::new();
        let server = Self {
            origin,
            tokens: tokens.clone(),
            records: records.clone(),
            failed: failed.clone(),
            stop: stop.clone(),
        };
        let upstream = upstream.map(|endpoint| {
            let url = url::Url::parse(endpoint).unwrap();
            format!("{}:{}", url.host_str().unwrap(), url.port().unwrap())
        });
        tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::<io::Result<()>>::new();
            loop {
                let (stream, _) = tokio::select! {
                    _ = stop.cancelled() => break,
                    accepted = listener.accept() => accepted.unwrap(),
                    Some(result) = tasks.join_next(), if !tasks.is_empty() => { result.unwrap().unwrap(); continue; },
                };
                let (tokens, records, failed, upstream) = (
                    tokens.clone(),
                    records.clone(),
                    failed.clone(),
                    upstream.clone(),
                );
                tasks
                    .spawn(async move { respond(stream, tokens, records, failed, upstream).await });
            }
            tasks.shutdown().await;
        });
        server
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn respond(
    mut stream: TcpStream,
    tokens: Arc<AtomicUsize>,
    records: Records,
    failed: Arc<AtomicBool>,
    upstream: Option<String>,
) -> io::Result<()> {
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
    let path = head.split_whitespace().nth(1).unwrap().to_owned();
    let headers: BTreeMap<_, _> = head
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .get("content-length")
        .map(|length| length.parse::<usize>().unwrap())
        .unwrap_or(0);
    while bytes.len() < end + length {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let body = if length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&bytes[end..end + length]).unwrap()
    };
    records
        .lock()
        .unwrap()
        .push((path.clone(), headers.clone(), body.clone()));
    assert!(headers.values().all(|value| !value.contains(SECRET)));
    if path == "/token" {
        assert_eq!(body, json!({"app_id":"fixture-app","app_secret":SECRET}));
        let revision = tokens.fetch_add(1, Ordering::SeqCst) + 1;
        let (status, body) = if failed.load(Ordering::SeqCst) {
            (503, json!({"reason":SECRET}))
        } else {
            (
                200,
                json!({"access_token":format!("application-token-{revision}"),"expires_in":2}),
            )
        };
        return reply(&mut stream, status, body).await;
    }
    let header = "x-access-token";
    assert_eq!(
        headers.get(header),
        Some(&format!(
            "application-token-{}",
            tokens.load(Ordering::SeqCst)
        ))
    );
    assert!(!body.to_string().contains(SECRET));
    if path == "/oversized" {
        let data = "x".repeat(256);
        let body = format!("event: message\ndata: {data}\n\n");
        return stream.write_all(format!("HTTP/1.1 200 Fixture\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await;
    }
    if let Some(upstream) = upstream {
        let mut peer = TcpStream::connect(upstream).await?;
        peer.write_all(&bytes).await?;
        let _ = tokio::io::copy_bidirectional(&mut stream, &mut peer).await;
        Ok(())
    } else {
        reply(&mut stream, 200, json!({"ok":true})).await
    }
}

async fn reply(stream: &mut TcpStream, status: u16, value: Value) -> io::Result<()> {
    let body = value.to_string();
    stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await
}
