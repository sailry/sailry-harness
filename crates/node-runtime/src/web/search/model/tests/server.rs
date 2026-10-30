//! Loopback transport fixture; no vendor endpoints or live credentials are used.
use serde_json::Value;
use std::collections::BTreeMap;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};

pub(super) struct Captured {
    pub route: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

pub(super) struct Server {
    pub endpoint: String,
    captured: oneshot::Receiver<Vec<Captured>>,
    task: JoinHandle<()>,
}

impl Server {
    pub async fn start(status: u16, content_type: &str, body: &str) -> Self {
        Self::script(vec![response(status, content_type, body)]).await
    }

    pub async fn sequence(replies: Vec<Value>) -> Self {
        Self::script(
            replies
                .into_iter()
                .map(|body| response(200, "application/json", &body.to_string()))
                .collect(),
        )
        .await
    }

    async fn script(replies: Vec<String>) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (captured, received) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for reply in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                requests.push(read(&mut socket).await);
                socket.write_all(reply.as_bytes()).await.unwrap();
            }
            captured.send(requests).ok();
        });
        Self {
            endpoint,
            captured: received,
            task,
        }
    }

    pub async fn captured(self) -> Captured {
        self.captures().await.remove(0)
    }

    pub async fn captures(self) -> Vec<Captured> {
        let captured = self.captured.await.unwrap();
        self.task.await.unwrap();
        captured
    }
}

fn response(status: u16, content_type: &str, body: &str) -> String {
    let content_type = if content_type.is_empty() {
        String::new()
    } else {
        format!("Content-Type: {content_type}\r\n")
    };
    format!(
        "HTTP/1.1 {status} Fixture\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn read(socket: &mut TcpStream) -> Captured {
    let mut buffer = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 1024];
        let size = socket.read(&mut chunk).await.unwrap();
        assert!(size > 0 && buffer.len() < 64 * 1024);
        buffer.extend_from_slice(&chunk[..size]);
        if let Some(end) = buffer.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let header = std::str::from_utf8(&buffer[..header_end]).unwrap();
    let mut lines = header.lines();
    let route = lines.next().unwrap().to_owned();
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect::<BTreeMap<_, _>>();
    let length = headers
        .get("content-length")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert!(length <= 1024 * 1024);
    while buffer.len() - header_end < length {
        let mut chunk = [0; 1024];
        let size = socket.read(&mut chunk).await.unwrap();
        assert!(size > 0);
        buffer.extend_from_slice(&chunk[..size]);
    }
    Captured {
        route,
        headers,
        body: serde_json::from_slice(&buffer[header_end..header_end + length]).unwrap(),
    }
}
