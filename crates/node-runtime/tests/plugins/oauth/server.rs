use sailry_link::CancellationToken;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) struct Server {
    pub endpoint: String,
    pub bodies: Arc<Mutex<Vec<String>>>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start(dingtalk: bool, expiring: bool) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let stop = CancellationToken::new();
        let (requests, closed) = (bodies.clone(), stop.clone());
        tokio::spawn(async move {
            loop {
                let mut socket = tokio::select! {
                    _ = closed.cancelled() => break,
                    accepted = listener.accept() => accepted.unwrap().0,
                };
                let mut bytes = Vec::new();
                let mut byte = [0; 1];
                while !bytes.ends_with(b"\r\n\r\n") {
                    if socket.read_exact(&mut byte).await.is_err() {
                        break;
                    }
                    bytes.push(byte[0]);
                    assert!(bytes.len() < 32 * 1024);
                }
                let headers = String::from_utf8(bytes).unwrap();
                assert!(headers.starts_with("POST /token HTTP/1.1"));
                let size: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_owned)
                    })
                    .unwrap()
                    .parse()
                    .unwrap();
                let mut bytes = vec![0; size];
                socket.read_exact(&mut bytes).await.unwrap();
                requests
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                let count = requests.lock().unwrap().len();
                let body = if dingtalk {
                    r#"{"accessToken":"private-access-marker","refreshToken":"private-refresh-marker","expireIn":7200}"#
                } else if expiring && count == 1 {
                    r#"{"access_token":"private-access-marker","refresh_token":"private-refresh-marker","expires_in":1,"token_type":"Bearer"}"#
                } else if expiring {
                    r#"{"access_token":"refreshed-access-marker","refresh_token":"rotated-refresh-marker","expires_in":3600,"token_type":"Bearer"}"#
                } else {
                    r#"{"access_token":"private-access-marker","refresh_token":"private-refresh-marker","expires_in":3600,"token_type":"Bearer"}"#
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        Self {
            endpoint,
            bodies,
            stop,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
