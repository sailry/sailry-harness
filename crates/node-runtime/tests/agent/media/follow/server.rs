//! One isolated endpoint holds Agent requests while serving its own generated images.
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinSet,
};

pub const KEY: &str = "isolated-follow-image-key";

#[derive(Clone, Copy, Debug)]
pub enum Outcome {
    Completed,
    Rejected(u16, &'static str),
    Disconnected,
    Invalid,
    Empty,
    Incomplete,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

impl Request {
    pub fn primary(&self) -> bool {
        self.body["stream"] == true
            || self.path.contains(":streamGenerateContent")
            || self.path.ends_with("/converse")
    }
}

pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<Request>>>,
    outcome: Arc<Mutex<Outcome>>,
    discovery: Arc<AtomicBool>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start() -> Self {
        Self::with_outcome(Outcome::Completed).await
    }

    pub async fn with_outcome(outcome: Outcome) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let records = requests.clone();
        let outcome = Arc::new(Mutex::new(outcome));
        let selected = outcome.clone();
        let discovery = Arc::new(AtomicBool::new(false));
        let discover = discovery.clone();
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
                        let discovery = discover.clone();
                        let outcome = *selected.lock().unwrap();
                        tasks.spawn(async move { let _ = respond(stream, records, outcome, discovery).await; });
                    }
                }
            }
            tasks.shutdown().await;
        });
        Self {
            endpoint,
            requests,
            outcome,
            discovery,
            stop,
        }
    }

    pub fn discover(&self) {
        self.discovery.store(true, Ordering::SeqCst);
    }

    pub fn set_outcome(&self, outcome: Outcome) {
        *self.outcome.lock().unwrap() = outcome;
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

async fn respond(
    mut stream: TcpStream,
    records: Arc<Mutex<Vec<Request>>>,
    outcome: Outcome,
    discovery: Arc<AtomicBool>,
) -> std::io::Result<()> {
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
    if header.starts_with("GET ") {
        stream
            .write_all(
                b"HTTP/1.1 426 Upgrade Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await?;
        return stream.shutdown().await;
    }
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
    let index = {
        let mut records = records.lock().unwrap();
        let index = records.len();
        records.push(request.clone());
        index
    };
    if primary {
        if discovery.swap(false, Ordering::SeqCst) {
            return discover(&mut stream, &request, index).await;
        }
        // StopTurn closes this request; generation uses another connection to this endpoint.
        let _ = stream.read(&mut [0]).await?;
        return Ok(());
    }
    if matches!(outcome, Outcome::Disconnected) {
        return stream.shutdown().await;
    }
    if let Outcome::Rejected(status, code) = outcome {
        let body = if path.contains("/publishers/google/models/") {
            let state = match status {
                400 | 422 => "INVALID_ARGUMENT",
                401 => "UNAUTHENTICATED",
                403 => "PERMISSION_DENIED",
                404 => "NOT_FOUND",
                405 | 501 => "UNIMPLEMENTED",
                429 => "RESOURCE_EXHAUSTED",
                500 => "INTERNAL",
                503 => "UNAVAILABLE",
                _ => "UNKNOWN",
            };
            json!({"error":{"code":status,"status":state,"message":"Fixture generation rejected",
                "details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":code,
                    "domain":"aiplatform.googleapis.com"}]}})
        } else {
            json!({"error":{"code":code,"message":"Fixture generation rejected"}})
        }
        .to_string();
        stream.write_all(format!("HTTP/1.1 {status} Fixture Rejection\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
        return stream.shutdown().await;
    }
    let png = super::super::server::PNG;
    let route = path.split('?').next().unwrap();
    let mut value = if path == "/v1/responses" {
        json!({"id":"image-response", "object":"response", "created_at":1,
            "status":"completed", "output":[{"id":"image-call", "type":"image_generation_call",
            "status":"completed", "result":png}], "usage":{"input_tokens":5,"output_tokens":4,"total_tokens":9}})
    } else if route.ends_with(":generateContent") {
        json!({"candidates":[{"content":{"role":"model","parts":[{"inlineData":{"mimeType":"image/png","data":png}}]},
            "finishReason":"STOP", "index":0}], "usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":4,"totalTokenCount":9}})
    } else if path == "/v1/images/generations" {
        json!({"data":[{"b64_json":png}], "usage":{"input_tokens":5,"output_tokens":4,"total_tokens":9}})
    } else {
        panic!("unexpected image fixture route: {path}");
    };
    match outcome {
        Outcome::Invalid => {
            if path == "/v1/responses" {
                value["output"][0]["result"] = json!("not-base64!");
            } else if route.ends_with(":generateContent") {
                value["candidates"][0]["content"]["parts"][0]["inlineData"]["data"] =
                    json!("not-base64!");
            } else {
                value["data"][0]["b64_json"] = json!("not-base64!");
            }
        }
        Outcome::Empty => {
            if path == "/v1/responses" {
                value["output"] = json!([]);
            } else if route.ends_with(":generateContent") {
                value["candidates"][0]["content"]["parts"] = json!([]);
            } else {
                value["data"] = json!([]);
            }
        }
        Outcome::Incomplete => {
            if path == "/v1/responses" {
                value["status"] = json!("incomplete");
            } else if route.ends_with(":generateContent") {
                value["candidates"][0]["finishReason"] = json!("MAX_TOKENS");
            } else {
                panic!("incomplete fixture requires a native image response");
            }
        }
        _ => {}
    }
    let body = serde_json::to_vec(&value).unwrap();
    stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

async fn discover(stream: &mut TcpStream, request: &Request, index: usize) -> std::io::Result<()> {
    let id = format!("discovery-response-{index}");
    let call = format!("discovery-call-{index}");
    let arguments = json!({
        "query": super::super::super::plugin_tool("media", "generate_image"),
        "limit": 1
    })
    .to_string();
    let model = &request.body["model"];
    let events = if request.path == "/v1/responses" {
        vec![
            json!({"type":"response.created","sequence_number":0,
                "response":{"id":id,"object":"response","created_at":0,
                    "model":model,"status":"in_progress","output":[]}}),
            json!({"type":"response.completed","sequence_number":1,
                "response":{"id":id,"object":"response","created_at":0,
                    "model":model,"status":"completed","output":[{
                        "id":call,"type":"function_call","call_id":call,
                        "name":"tool_search","arguments":arguments,"status":"completed"}]}}),
        ]
    } else {
        assert_eq!(request.path, "/v1/chat/completions");
        vec![
            json!({"id":id,"object":"chat.completion.chunk","created":0,"model":model,
            "choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{
                "index":0,"id":call,"type":"function","function":{
                    "name":"tool_search","arguments":arguments}}]},"finish_reason":"tool_calls"}]}),
        ]
    };
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
        )
        .await?;
    for event in events {
        stream
            .write_all(format!("data: {event}\n\n").as_bytes())
            .await?;
    }
    stream.write_all(b"data: [DONE]\n\n").await?;
    stream.shutdown().await
}
