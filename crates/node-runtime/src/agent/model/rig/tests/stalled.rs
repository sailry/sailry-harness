use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::JoinHandle,
};

pub(super) struct Server {
    pub endpoint: String,
    started: Arc<Notify>,
    closed: Arc<Notify>,
    task: JoinHandle<()>,
}

impl Server {
    pub async fn start(prefix: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let started = Arc::new(Notify::new());
        let closed = Arc::new(Notify::new());
        let received = started.clone();
        let disconnected = closed.clone();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let chunk = format!("{:x}\r\n{prefix}\r\n", prefix.len());
            socket.write_all(chunk.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
            received.notify_one();
            let mut input = [0; 1];
            match socket.read(&mut input).await {
                Ok(0) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ) => {}
                result => panic!("expected cancelled HTTP connection, got {result:?}"),
            }
            disconnected.notify_one();
        });
        Self {
            endpoint,
            started,
            closed,
            task,
        }
    }

    pub async fn ready(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.started.notified())
            .await
            .unwrap();
    }

    pub async fn closed(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.closed.notified())
            .await
            .unwrap();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn read_request(socket: &mut TcpStream) {
    let mut input = Vec::new();
    let boundary = loop {
        let mut chunk = [0; 4096];
        let received = socket.read(&mut chunk).await.unwrap();
        assert!(received > 0, "request ended before headers");
        input.extend_from_slice(&chunk[..received]);
        if let Some(position) = input.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break position + 4;
        }
    };
    let headers = std::str::from_utf8(&input[..boundary]).unwrap();
    let length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .unwrap()
        .1
        .trim()
        .parse::<usize>()
        .unwrap();
    while input.len() < boundary + length {
        let mut chunk = [0; 4096];
        let received = socket.read(&mut chunk).await.unwrap();
        assert!(received > 0, "request ended before body");
        input.extend_from_slice(&chunk[..received]);
    }
}
