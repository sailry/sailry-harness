//! One isolated endpoint holds Agent requests while serving its own generated images.
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

pub const KEY: &str = "isolated-follow-image-key";

#[derive(Clone, Debug)]
pub struct Request {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

impl Request {
    pub fn primary(&self) -> bool {
        self.body["stream"] == true || self.path.contains(":streamGenerateContent")
    }
}

pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let records = requests.clone();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        tokio::spawn(async move {
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    _ = stopped.cancelled() => break,
                    _ = tasks.join_next(), if !tasks.is_empty() => {},
                    result = listener.accept() => {
                        let (stream, _) = result.unwrap();
                        let records = records.clone();
                        tasks.spawn(async move { let _ = respond(stream, records).await; });
                    }
                }
            }
            tasks.shutdown().await;
        });
        Self {
            endpoint,
            requests,
            stop,
        }
    }

    pub async fn wait_primary(&self, count: usize) -> Result<(), tokio::time::error::Elapsed> {
        tokio::time::timeout(Duration::from_secs(10), async {
            while self
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.primary())
                .count()
                < count
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn respond(mut stream: TcpStream, records: Arc<Mutex<Vec<Request>>>) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    let end = loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 2 * 1024 * 1024);
        if let Some(end) = bytes.windows(4).position(|chunk| chunk == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..end]).unwrap();
    let path = header
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    let headers: BTreeMap<_, _> = header
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers["content-length"].parse().unwrap();
    assert!(length < 2 * 1024 * 1024);
    while bytes.len() < end + length {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Ok(());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let body: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
    let request = Request {
        path: path.clone(),
        headers,
        body,
    };
    let primary = request.primary();
    records.lock().unwrap().push(request);
    if primary {
        // StopTurn closes this request; generation uses another connection to this endpoint.
        let _ = stream.read(&mut [0]).await?;
        return Ok(());
    }
    let png = super::super::server::PNG;
    let value = if path == "/v1/responses" {
        json!({"id":"image-response", "object":"response", "created_at":1,
            "status":"completed", "output":[{"id":"image-call", "type":"image_generation_call",
            "status":"completed", "result":png}], "usage":{"input_tokens":5,"output_tokens":4,"total_tokens":9}})
    } else if path.ends_with(":generateContent") {
        json!({"candidates":[{"content":{"role":"model","parts":[{"inlineData":{"mimeType":"image/png","data":png}}]},
            "finishReason":"STOP", "index":0}], "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":4,"totalTokenCount":9}})
    } else if path == "/v1/images/generations" {
        json!({"data":[{"b64_json":png}], "usage":{"input_tokens":5,"output_tokens":4,"total_tokens":9}})
    } else {
        panic!("unexpected image fixture route: {path}");
    };
    let body = serde_json::to_vec(&value).unwrap();
    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}
