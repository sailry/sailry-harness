use base64::Engine as _;
use sailry_link::CancellationToken;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+j3ioAAAAASUVORK5CYII=";
pub fn image() -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(PNG)
        .unwrap()
}

pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<(String, Value)>>>,
    #[allow(
        dead_code,
        reason = "Only model service fixtures inspect captured headers"
    )]
    pub headers: Arc<Mutex<Vec<std::collections::BTreeMap<String, String>>>>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let records = requests.clone();
        let headers = Arc::new(Mutex::new(Vec::new()));
        let captured_headers = headers.clone();
        let base = endpoint.clone();
        let stop = CancellationToken::new();
        let stopped = stop.clone();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = tokio::select! { _ = stopped.cancelled() => break, accepted = listener.accept() => accepted.unwrap() };
                let mut bytes = Vec::new();
                let (header, offset, length) = loop {
                    let mut chunk = [0u8; 8192];
                    let count = stream.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        break (String::new(), 0, 0);
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let header = String::from_utf8(bytes[..offset].to_vec()).unwrap();
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|value| value.parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (header, offset + 4, length);
                    }
                };
                if header.is_empty() {
                    continue;
                }
                while bytes.len() < offset + length {
                    let mut chunk = [0u8; 8192];
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                let path = header
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let input: Value = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[offset..offset + length]).unwrap()
                };
                captured_headers.lock().unwrap().push(
                    header
                        .lines()
                        .skip(1)
                        .filter_map(|line| {
                            line.split_once(':').map(|(key, value)| {
                                (key.to_ascii_lowercase(), value.trim().to_owned())
                            })
                        })
                        .collect(),
                );
                let index = {
                    let mut records = records.lock().unwrap();
                    let index = records.len();
                    records.push((path.to_owned(), input.clone()));
                    index
                };
                let value = match path {
                    "/v1/chat/completions" => {
                        assert_eq!(input["stream"], true);
                        json!({"id":format!("vision-{index}"), "object":"chat.completion.chunk", "created":0,"model":input["model"],"choices":[{"index":0,"delta":{"role":"assistant","content":"One bright pixel"},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":4,"total_tokens":9}})
                    }
                    "/v1/images/generations" => {
                        let image = if input["prompt"] == "A pixel" {
                            json!({"url":format!("{base}/generated.png")})
                        } else {
                            json!({"b64_json":if input["prompt"] == "Invalid artifact" { "aW52YWxpZA==" } else { PNG }})
                        };
                        json!({"data":[image],"usage":{"input_tokens":50,"output_tokens":60,"total_tokens":110,"input_tokens_details":{"text_tokens":10,"image_tokens":40}}})
                    }
                    "/v1/videos" => {
                        json!({"id":if input["prompt"] == "pending" { "pending" } else { "video_fixture" },"status":"queued"})
                    }
                    "/v1/videos/video_fixture" => {
                        json!({"id":"video_fixture","status":"completed"})
                    }
                    "/v1/videos/pending" => json!({"id":"pending","status":"in_progress"}),
                    "/v1/videos/video_fixture/content" => Value::Null,
                    "/v1/generated.png" => Value::Null,
                    _ => panic!("unexpected media path: {path}"),
                };
                let body = if path == "/v1/chat/completions" {
                    let mut delta = value.clone();
                    delta["choices"][0]["finish_reason"] = Value::Null;
                    delta.as_object_mut().unwrap().remove("usage");
                    let mut terminal = value;
                    terminal["choices"][0]["delta"] = json!({});
                    format!("data: {delta}\n\ndata: {terminal}\n\ndata: [DONE]\n\n").into_bytes()
                } else if path.ends_with("generated.png") {
                    image()
                } else if path.ends_with("/content") {
                    b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom".to_vec()
                } else {
                    serde_json::to_vec(&value).unwrap()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    if path == "/v1/chat/completions" {
                        "text/event-stream"
                    } else if path.ends_with("generated.png") {
                        "image/png"
                    } else if path.ends_with("/content") {
                        "video/mp4"
                    } else {
                        "application/json"
                    },
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            }
        });
        Self {
            endpoint,
            requests,
            headers,
            stop,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
