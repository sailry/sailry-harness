use sailry_link::CancellationToken;
use sailry_protocol::conversation::ModelApi;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

pub enum Reply {
    Json(Value),
    Hold,
    Raw(String),
    Delayed(Arc<tokio::sync::Notify>, Value),
}

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
    closed: Arc<AtomicUsize>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start(
        api: ModelApi,
        reply: impl Fn(&str) -> Reply + Send + Sync + 'static,
    ) -> Self {
        Self::start_with_request(api, move |request| {
            assert_eq!(request.method, "GET");
            assert!(request.body.is_empty());
            reply(&request.path)
        })
        .await
    }

    pub async fn start_with_request(
        api: ModelApi,
        reply: impl Fn(&Request) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let suffix = match api {
            ModelApi::Anthropic => "",
            ModelApi::Gemini => "/v1beta",
            _ => "/v1",
        };
        let endpoint = format!("http://{}{suffix}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicUsize::new(0));
        let stop = CancellationToken::new();
        let records = requests.clone();
        let count = closed.clone();
        let cancelled = stop.clone();
        let reply = Arc::new(reply);
        tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    _ = cancelled.cancelled() => break,
                    _ = tasks.join_next(), if !tasks.is_empty() => {},
                    result = listener.accept() => {
                        let (stream, _) = result.unwrap();
                        let records = records.clone();
                        let count = count.clone();
                        let reply = reply.clone();
                        tasks.spawn(async move { let _ = respond(stream, records, count, reply).await; });
                    }
                }
            }
            tasks.shutdown().await;
        });
        Self {
            endpoint,
            requests,
            closed,
            stop,
        }
    }
    pub async fn wait_requests(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.requests.lock().unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("provider request deadline");
    }
    pub async fn wait_closed(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.closed.load(Ordering::SeqCst) < count {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("provider connection release deadline");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn respond(
    mut stream: TcpStream,
    records: Arc<Mutex<Vec<Request>>>,
    closed: Arc<AtomicUsize>,
    reply: Arc<impl Fn(&Request) -> Reply>,
) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    while !bytes.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 64 * 1024);
    }
    let boundary = bytes
        .windows(4)
        .position(|chunk| chunk == b"\r\n\r\n")
        .unwrap()
        + 4;
    let header = std::str::from_utf8(&bytes[..boundary]).unwrap();
    let method = header.split_whitespace().next().unwrap().to_owned();
    let path = header
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    let headers: BTreeMap<String, String> = header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .get("content-length")
        .map_or(0, |value| value.parse::<usize>().unwrap());
    assert!(length <= 64 * 1024);
    while bytes.len() < boundary + length {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let request = Request {
        method,
        path,
        headers,
        body: String::from_utf8(bytes[boundary..boundary + length].to_vec()).unwrap(),
    };
    let response = reply(&request);
    records.lock().unwrap().push(request);
    match response {
        Reply::Hold => {
            if stream.read(&mut buffer).await? == 0 {
                closed.fetch_add(1, Ordering::SeqCst);
            }
        }
        Reply::Delayed(release, value) => {
            release.notified().await;
            write_json(&mut stream, value).await?;
        }
        Reply::Raw(value) => {
            stream.write_all(value.as_bytes()).await?;
            stream.shutdown().await?;
        }
        Reply::Json(value) => write_json(&mut stream, value).await?,
    }
    Ok(())
}

async fn write_json(stream: &mut TcpStream, value: Value) -> std::io::Result<()> {
    let body = value.to_string();
    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}
