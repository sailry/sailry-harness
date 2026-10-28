use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
};

pub(super) struct Request {
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

pub(super) struct Server {
    pub endpoint: String,
    requests: Arc<Mutex<Vec<Request>>>,
    task: Option<JoinHandle<()>>,
}

impl Server {
    pub async fn start(replies: Vec<String>) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut input = Vec::new();
                let boundary = loop {
                    let mut chunk = [0; 4096];
                    let length = socket.read(&mut chunk).await.unwrap();
                    assert!(length > 0, "request ended before headers");
                    input.extend_from_slice(&chunk[..length]);
                    if let Some(position) = input.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                    {
                        break position + 4;
                    }
                };
                let encoded = std::str::from_utf8(&input[..boundary]).unwrap();
                let mut lines = encoded.lines();
                let path = lines
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let headers = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(key, value)| (key.to_ascii_lowercase(), value.trim().to_owned()))
                    .collect::<BTreeMap<_, _>>();
                let length: usize = headers["content-length"].parse().unwrap();
                while input.len() < boundary + length {
                    let mut chunk = [0; 4096];
                    let received = socket.read(&mut chunk).await.unwrap();
                    assert!(received > 0, "request ended before body");
                    input.extend_from_slice(&chunk[..received]);
                }
                let body = serde_json::from_slice(&input[boundary..boundary + length]).unwrap();
                captured.lock().unwrap().push(Request {
                    path,
                    headers,
                    body,
                });
                if !reply.starts_with("HTTP/") {
                    let headers = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        reply.len()
                    );
                    socket.write_all(headers.as_bytes()).await.unwrap();
                }
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            task: Some(task),
        }
    }

    pub async fn finish(mut self) -> Vec<Request> {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.task.take().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        std::mem::take(&mut *self.requests.lock().unwrap())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
