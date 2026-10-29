//! One billable fixture observes native HTTP requests without changing GDK retries.
use futures::StreamExt;
use sailry_link::CancellationToken;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

#[derive(Clone, Default, serde::Serialize)]
pub(super) struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

#[derive(Clone)]
pub(super) struct Request {
    pub body: Value,
    pub encoded: String,
    pub status: Option<u16>,
    pub transport_failed: bool,
    pub completion: Option<&'static str>,
    pub output_limited: bool,
    pub provider_usage: Usage,
}

pub(super) struct Relay {
    pub endpoint: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: CancellationToken,
}

impl Relay {
    pub async fn start(endpoint: &str) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let origin = reqwest::Url::parse(endpoint).expect("valid captured endpoint");
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .expect("relay HTTP client");
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut local = origin.clone();
        local.set_scheme("http").unwrap();
        local.set_host(Some("127.0.0.1")).unwrap();
        local.set_port(Some(address.port())).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = CancellationToken::new();
        let cancelled = stop.clone();
        tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = cancelled.cancelled() => break,
                    _ = connections.join_next(), if !connections.is_empty() => {},
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let (client, origin, requests) = (client.clone(), origin.clone(), captured.clone());
                        connections.spawn(async move { forward(stream, &client, &origin, &requests).await });
                    }
                }
            }
            connections.shutdown().await;
        });
        Self {
            endpoint: local.into(),
            requests,
            stop,
        }
    }

    pub fn snapshot(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn forward(
    mut socket: TcpStream,
    client: &reqwest::Client,
    origin: &reqwest::Url,
    requests: &Mutex<Vec<Request>>,
) -> std::io::Result<()> {
    let mut input = Vec::new();
    let boundary = loop {
        let mut chunk = [0; 8192];
        let length = socket.read(&mut chunk).await?;
        if length == 0 {
            return Ok(());
        }
        input.extend_from_slice(&chunk[..length]);
        assert!(
            input.len() <= 2 * 1024 * 1024,
            "synthetic request is bounded"
        );
        if let Some(position) = input.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let header = std::str::from_utf8(&input[..boundary]).unwrap();
    let mut lines = header.lines();
    let mut first = lines.next().unwrap().split_whitespace();
    assert_eq!(first.next(), Some("POST"));
    let target = origin
        .join(first.next().unwrap())
        .expect("captured request path");
    assert_eq!(target.origin(), origin.origin());
    let mut headers = reqwest::header::HeaderMap::new();
    let mut length = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').unwrap();
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "host" | "content-length" | "connection" | "transfer-encoding"
        ) {
            continue;
        }
        headers.append(
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            reqwest::header::HeaderValue::from_str(value.trim()).unwrap(),
        );
    }
    let length = length.expect("native JSON request length");
    assert!(length <= 2 * 1024 * 1024, "synthetic request is bounded");
    while input.len() < boundary + length {
        let mut chunk = [0; 8192];
        let received = socket.read(&mut chunk).await?;
        if received == 0 {
            return Ok(());
        }
        input.extend_from_slice(&chunk[..received]);
    }
    let encoded = String::from_utf8(input[boundary..boundary + length].to_vec()).unwrap();
    let body = serde_json::from_str(&encoded).expect("synthetic provider JSON");
    let index = {
        let mut requests = requests.lock().unwrap();
        let index = requests.len();
        requests.push(Request {
            body,
            encoded: encoded.clone(),
            status: None,
            transport_failed: false,
            completion: None,
            output_limited: false,
            provider_usage: Usage::default(),
        });
        index
    };
    let result = async {
        let response = client
            .post(target)
            .headers(headers)
            .body(encoded)
            .send()
            .await
            .map_err(|_| std::io::Error::other("relay upstream request failed"))?;
        let status = response.status();
        requests.lock().unwrap()[index].status = Some(status.as_u16());
        let mut output = format!(
            "HTTP/1.1 {} {}\r\nConnection: close\r\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Response")
        );
        for (name, value) in response.headers() {
            if matches!(
                name.as_str(),
                "content-length" | "connection" | "transfer-encoding"
            ) {
                continue;
            }
            output.push_str(name.as_str());
            output.push_str(": ");
            output.push_str(
                value
                    .to_str()
                    .map_err(|_| std::io::Error::other("relay response header is invalid"))?,
            );
            output.push_str("\r\n");
        }
        output.push_str("\r\n");
        socket.write_all(output.as_bytes()).await?;
        let mut chunks = response.bytes_stream();
        let mut pending = Vec::new();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.map_err(|_| std::io::Error::other("relay upstream stream failed"))?;
            completion(&mut pending, &chunk, &mut requests.lock().unwrap()[index]);
            socket.write_all(&chunk).await?;
        }
        socket.shutdown().await
    }
    .await;
    if result.is_err() {
        requests.lock().unwrap()[index].transport_failed = true;
    }
    result
}

fn completion(pending: &mut Vec<u8>, chunk: &[u8], request: &mut Request) {
    pending.extend_from_slice(chunk);
    assert!(
        pending.len() <= 2 * 1024 * 1024,
        "synthetic SSE event is bounded"
    );
    while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
        let line: Vec<_> = pending.drain(..=end).collect();
        let Some(data) = line.strip_prefix(b"data:") else {
            continue;
        };
        let Ok(event) = serde_json::from_slice::<Value>(data) else {
            continue;
        };
        if matches!(
            event["type"].as_str(),
            Some("response.completed" | "response.incomplete" | "response.failed")
        ) {
            let usage = &event["response"]["usage"];
            request.provider_usage = Usage {
                input_tokens: usage["input_tokens"].as_u64(),
                output_tokens: usage["output_tokens"].as_u64(),
                cached_tokens: usage["input_tokens_details"]["cached_tokens"].as_u64(),
                reasoning_tokens: usage["output_tokens_details"]["reasoning_tokens"].as_u64(),
            };
        }
        request.output_limited |=
            event["response"]["incomplete_details"]["reason"] == "max_output_tokens";
        request.completion = match event["type"].as_str() {
            Some("response.completed") => Some(match event["response"]["status"].as_str() {
                Some("incomplete") => "incomplete",
                Some("failed") => "failed",
                _ => "completed",
            }),
            Some("response.incomplete") => Some("incomplete"),
            Some("response.failed") => Some("failed"),
            _ => request.completion,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn forwards_stream_without_replay() {
        let server = crate::agent_support::Server::start(false).await;
        let relay = Relay::start(&server.endpoint).await;
        let body = json!({"model":"fixture-a","stream":true,"input":[{"role":"user","content":[{"type":"input_text","text":"Synthetic relay proof"}]}]});
        let response = reqwest::Client::new()
            .post(format!(
                "{}/responses",
                relay.endpoint.trim_end_matches('/')
            ))
            .bearer_auth("synthetic-relay-key")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let mut chunks = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = chunks.next().await {
            bytes.extend_from_slice(&chunk.unwrap());
        }
        assert!(String::from_utf8(bytes).unwrap().contains("[DONE]"));
        let requests = relay.snapshot();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].body, body);
        assert_eq!(requests[0].status, Some(200));
        assert!(!requests[0].transport_failed);
        assert_eq!(requests[0].completion, Some("completed"));
        assert!(!requests[0].output_limited);
        assert_eq!(server.requests.lock().unwrap().as_slice(), [body]);
        assert_eq!(
            server.authorization.lock().unwrap().as_slice(),
            [Some("Bearer synthetic-relay-key".into())]
        );
    }
}
