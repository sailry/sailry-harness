use sailry_link::CancellationToken;
use std::{collections::BTreeMap, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, Notify},
};

pub(crate) struct Sources {
    pub endpoint: String,
    pub requests: Arc<Mutex<Vec<String>>>,
    pub release: Arc<Notify>,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}
impl Sources {
    pub async fn new(routes: BTreeMap<String, Vec<u8>>, hold: Option<String>) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let release = Arc::new(Notify::new());
        let stop = CancellationToken::new();
        let routes = Arc::new(routes);
        let cancelled = stop.clone();
        let observed = requests.clone();
        let gate = release.clone();
        let task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {biased;_ = cancelled.cancelled()=>break,accepted=listener.accept()=>accepted};
                let (mut stream, _) = accepted.unwrap();
                let routes = routes.clone();
                let requests = observed.clone();
                let gate = gate.clone();
                let hold = hold.clone();
                let stop = cancelled.clone();
                tokio::spawn(async move {
                    let operation = async {
                        let mut headers = Vec::new();
                        while !headers.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            if stream.read_exact(&mut byte).await.is_err() || headers.len() >= 8192
                            {
                                return;
                            }
                            headers.push(byte[0]);
                        }
                        let path = String::from_utf8_lossy(&headers)
                            .split_whitespace()
                            .nth(1)
                            .unwrap()
                            .to_owned();
                        requests.lock().await.push(path.clone());
                        let bytes = routes.get(&path);
                        let status = if bytes.is_some() {
                            "200 OK"
                        } else {
                            "404 Not Found"
                        };
                        let bytes = bytes.map(Vec::as_slice).unwrap_or(b"missing");
                        if stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).as_bytes()).await.is_err() {return;}
                        if hold.as_ref() == Some(&path) {
                            gate.notified().await;
                        }
                        let _ = stream.write_all(bytes).await;
                    };
                    tokio::select! {biased;_ = stop.cancelled()=>{},_ = operation=>{}}
                });
            }
        });
        Self {
            endpoint,
            requests,
            release,
            stop,
            task,
        }
    }
    pub async fn requested(&self, path: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if self
                    .requests
                    .lock()
                    .await
                    .iter()
                    .any(|request| request == path)
                {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
