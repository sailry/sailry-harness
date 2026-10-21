use sailry_link::CancellationToken;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU16, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub struct Server {
    pub origin: String,
    pub requests: Arc<Mutex<Vec<(String, String)>>>,
    pub held: CancellationToken,
    pub disconnected: CancellationToken,
    pub status: Arc<AtomicU16>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start() -> Self {
        Self::respond(None, None).await
    }

    pub(in crate::plugins) async fn respond(
        body: Option<String>,
        held_path: Option<String>,
    ) -> Self {
        Self::build(body, held_path, Vec::new()).await
    }

    pub(in crate::plugins) async fn build(
        body: Option<String>,
        held_path: Option<String>,
        headers: Vec<(String, String)>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let held = CancellationToken::new();
        let disconnected = CancellationToken::new();
        let stop = CancellationToken::new();
        let server = Self {
            origin,
            requests: requests.clone(),
            held: held.clone(),
            disconnected: disconnected.clone(),
            status: Arc::new(AtomicU16::new(0)),
            stop: stop.clone(),
        };
        let status = server.status.clone();
        tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (mut stream, _) = tokio::select! {
                    _ = stop.cancelled() => break,
                    accepted = listener.accept() => accepted.unwrap(),
                };
                let (records, held, disconnected) =
                    (requests.clone(), held.clone(), disconnected.clone());
                let (body, held_path) = (body.clone(), held_path.clone());
                let headers = headers.clone();
                let status = status.clone();
                connections.spawn(async move {
                    let mut bytes = Vec::new();
                    let (header, offset, length) = loop {
                        let mut chunk = [0; 8192];
                        let count = stream.read(&mut chunk).await.unwrap();
                        if count == 0 { return; }
                        bytes.extend_from_slice(&chunk[..count]);
                        if let Some(offset) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                            let header = String::from_utf8(bytes[..offset].to_vec()).unwrap();
                            let length = header.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ").map(|length| length.parse::<usize>().unwrap())).unwrap_or(0);
                            break (header, offset + 4, length);
                        }
                    };
                    while bytes.len() < offset + length {
                        let mut chunk = [0; 8192];
                        let count = stream.read(&mut chunk).await.unwrap();
                        if count == 0 { return; }
                        bytes.extend_from_slice(&chunk[..count]);
                    }
                    let path = header.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_string();
                    let authorized = headers.iter().all(|(name, expected)| {
                        header.lines().skip(1).any(|line| {
                            line.split_once(':').is_some_and(|(key, value)| {
                                key.eq_ignore_ascii_case(name) && value.trim() == expected
                            })
                        })
                    });
                    records.lock().unwrap().push((header, String::from_utf8(bytes[offset..offset + length].to_vec()).unwrap()));
                    if !authorized {
                        let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        return;
                    }
                    let status = status.load(Ordering::SeqCst);
                    if status != 0 {
                        let response = format!("HTTP/1.1 {status} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        let _ = stream.write_all(response.as_bytes()).await;
                        return;
                    }
                    match path.as_str() {
                        "/drop" => {},
                        path if path == "/hold" || held_path.as_deref() == Some(path) => {
                            held.cancel();
                            let _ = stream.read(&mut [0; 1]).await;
                            disconnected.cancel();
                        },
                        "/large" => {
                            let body = "x".repeat(sailry_protocol::plugin::http::MAX_BODY + 1);
                            let response = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len());
                            let _ = stream.write_all(response.as_bytes()).await;
                        },
                        "/redirect" => {
                            let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        },
                        "/unauthorized" => {
                            let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                        },
                        "/public" => {
                            let body = r#"{"id":"actual-record"}"#;
                            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                            let _ = stream.write_all(response.as_bytes()).await;
                        },
                        _ => {
                            if let Some(body) = body {
                                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                                let _ = stream.write_all(response.as_bytes()).await;
                            } else {
                                let _ = stream.write_all(b"HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: 5\r\nConnection: close\r\n\r\nsaved").await;
                            }
                        }
                    }
                });
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        server
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
