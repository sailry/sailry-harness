use sailry_link::CancellationToken;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

#[path = "formatted.rs"]
mod formatted;
#[path = "transport.rs"]
mod transport;

pub struct Server {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<(String, Value)>>>,
    stop: CancellationToken,
}

impl Server {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = CancellationToken::new();
        let server = Self {
            endpoint,
            requests: requests.clone(),
            stop: stop.clone(),
        };
        tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (mut stream, _) = tokio::select! {
                    _ = stop.cancelled() => break,
                    accepted = listener.accept() => accepted.unwrap(),
                };
                let requests = requests.clone();
                connections.spawn(async move {
                    let Some(request) = transport::read(&mut stream).await else {
                        return;
                    };
                    assert_eq!(request.path, "/mcp");
                    let (status, body) = formatted::response(&request.method, &request.body, false);
                    requests
                        .lock()
                        .unwrap()
                        .push((request.header, request.body));
                    transport::respond(&mut stream, status, None, &body, None).await;
                });
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        server
    }

    pub fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(header, _)| {
                header
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .split('?')
                    .next()
                    == Some(path)
            })
            .count()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
