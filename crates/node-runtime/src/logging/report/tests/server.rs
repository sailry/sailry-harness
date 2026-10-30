use sentry::{Envelope, protocol::EnvelopeItem};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
    sync::mpsc,
};

pub(super) struct Server {
    pub dsn: String,
    received: mpsc::Receiver<Value>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    pub async fn new() -> Self {
        Self::start(false, 200).await
    }
    pub async fn paused() -> Self {
        Self::start(true, 200).await
    }
    pub async fn rejected() -> Self {
        Self::start(false, 403).await
    }

    async fn start(paused: bool, status: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, received) = mpsc::channel(64);
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut bytes = Vec::new();
                let end = loop {
                    let mut part = [0_u8; 4096];
                    let count = socket.read(&mut part).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&part[..count]);
                    if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                        break end + 4;
                    }
                    assert!(bytes.len() < 65536);
                };
                let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
                assert!(header.starts_with("POST /api/1/envelope/ "));
                assert!(
                    header
                        .to_ascii_lowercase()
                        .contains("x-sentry-auth: sentry ")
                );
                let length = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                while bytes.len() < end + length {
                    let mut part = vec![0; (end + length - bytes.len()).min(65536)];
                    let count = socket.read(&mut part).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&part[..count]);
                }
                let envelope = Envelope::from_slice(&bytes[end..end + length]).unwrap();
                let mut record = json!({});
                for item in envelope.items() {
                    match item {
                        EnvelopeItem::Event(event) => {
                            record["event"] = serde_json::to_value(event).unwrap()
                        }
                        EnvelopeItem::Attachment(attachment) => {
                            record["attachment"] =
                                String::from_utf8(attachment.buffer.clone()).unwrap().into();
                            assert_eq!(attachment.filename, "diagnostics.jsonl");
                        }
                        _ => panic!("unexpected envelope item"),
                    }
                }
                sender.send(record).await.unwrap();
                if paused {
                    std::future::pending::<()>().await;
                }
                let body = "PRIVATE_UPSTREAM_RESPONSE";
                socket
                    .write_all(format!("HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())
                    .await
                    .unwrap();
            }
        });
        Self {
            dsn: format!("http://synthetic@{address}/1"),
            received,
            task,
        }
    }

    pub async fn next(&mut self) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(3), self.received.recv())
            .await
            .unwrap()
            .unwrap()
    }

    pub fn empty(&mut self) -> bool {
        self.received.try_recv().is_err()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
