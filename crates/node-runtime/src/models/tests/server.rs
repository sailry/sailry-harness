use super::*;
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
    task::{JoinHandle, JoinSet},
};

#[derive(Clone, Debug)]
pub(super) struct Request {
    pub path: String,
    pub query: BTreeMap<String, String>,
}

pub(super) struct Reply {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
    declared: Option<usize>,
    hold: bool,
    chunked: bool,
}

impl Reply {
    pub fn body(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            body,
            headers: Vec::new(),
            declared: None,
            hold: false,
            chunked: false,
        }
    }

    pub fn json(value: Value) -> Self {
        Self::body(serde_json::to_vec(&value).unwrap())
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::body(Vec::new())
        }
    }

    pub fn header(mut self, name: &str, value: String) -> Self {
        self.headers.push((name.into(), value));
        self
    }

    pub fn oversized(mut self, size: usize) -> Self {
        self.declared = Some(size);
        self
    }

    pub fn hold() -> Self {
        Self {
            hold: true,
            declared: Some(1),
            ..Self::body(Vec::new())
        }
    }

    pub fn chunked(mut self) -> Self {
        self.chunked = true;
        self
    }
}

pub(super) struct Server {
    url: reqwest::Url,
    requests: Arc<Mutex<Vec<Request>>>,
    received: Arc<Notify>,
    sent: Arc<Notify>,
    errors: Arc<Mutex<Vec<String>>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Server {
    pub async fn start(
        router: impl Fn(&Request, &reqwest::Url) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url: reqwest::Url = format!("http://{}/", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::new(Notify::new());
        let sent = Arc::new(Notify::new());
        let errors = Arc::new(Mutex::new(Vec::new()));
        let stop = CancellationToken::new();
        let stopping = stop.clone();
        let recorded = requests.clone();
        let observed = received.clone();
        let written = sent.clone();
        let failed = errors.clone();
        let base = url.clone();
        let router = Arc::new(router);
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                let (mut socket, _) = tokio::select! {
                    biased;
                    _ = stopping.cancelled() => break,
                    connection = listener.accept() => connection.unwrap(),
                    _ = connections.join_next(), if !connections.is_empty() => continue,
                };
                let router = router.clone();
                let base = base.clone();
                let recorded = recorded.clone();
                let observed = observed.clone();
                let written = written.clone();
                let failed = failed.clone();
                let closed = stopping.clone();
                connections.spawn(async move {
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        let read = tokio::select! {
                            biased;
                            _ = closed.cancelled() => return,
                            read = socket.read(&mut byte) => read.unwrap(),
                        };
                        if read == 0 {
                            return;
                        }
                        header.extend_from_slice(&byte);
                        assert!(header.len() < 16384);
                    }
                    let line = std::str::from_utf8(&header)
                        .unwrap()
                        .lines()
                        .next()
                        .unwrap();
                    assert_eq!(line.split_whitespace().next(), Some("GET"));
                    let url = base.join(line.split_whitespace().nth(1).unwrap()).unwrap();
                    let request = Request {
                        path: url.path().into(),
                        query: url
                            .query_pairs()
                            .map(|(key, value)| (key.into_owned(), value.into_owned()))
                            .collect(),
                    };
                    recorded.lock().unwrap().push(request.clone());
                    observed.notify_one();
                    let reply = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        router(&request, &base)
                    })) {
                        Ok(reply) => reply,
                        Err(_) => {
                            failed
                                .lock()
                                .unwrap()
                                .push("fixture router assertion failed".into());
                            return;
                        }
                    };
                    let mut header =
                        format!("HTTP/1.1 {} Fixture\r\nConnection: close\r\n", reply.status);
                    if reply.chunked {
                        header.push_str("Transfer-Encoding: chunked\r\n");
                    } else {
                        header.push_str(&format!(
                            "Content-Length: {}\r\n",
                            reply.declared.unwrap_or(reply.body.len())
                        ));
                    }
                    for (name, value) in reply.headers {
                        header.push_str(&format!("{name}: {value}\r\n"));
                    }
                    header.push_str("\r\n");
                    if socket.write_all(header.as_bytes()).await.is_err() {
                        return;
                    }
                    written.notify_one();
                    if reply.hold {
                        closed.cancelled().await;
                        return;
                    }
                    if reply.chunked {
                        for chunk in reply.body.chunks(8192) {
                            if socket
                                .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                                .await
                                .is_err()
                                || socket.write_all(chunk).await.is_err()
                                || socket.write_all(b"\r\n").await.is_err()
                            {
                                return;
                            }
                        }
                        let _ = socket.write_all(b"0\r\n\r\n").await;
                    } else {
                        let _ = socket.write_all(&reply.body).await;
                    }
                });
            }
        });
        Self {
            url,
            requests,
            received,
            sent,
            errors,
            stop,
            task,
        }
    }

    pub fn http(&self) -> Http {
        Http::fixture(self.url.clone())
    }

    pub fn http_at(&self, path: &str) -> Http {
        Http::fixture(self.url.join(path).unwrap())
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    pub async fn wait(&self, count: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while self.requests.lock().unwrap().len() < count {
                self.received.notified().await;
            }
        })
        .await
        .expect("fixture request arrived");
    }

    pub async fn headers(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.sent.notified())
            .await
            .expect("fixture response headers were written");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
        if !std::thread::panicking() {
            assert!(
                self.errors.lock().unwrap().is_empty(),
                "fixture router assertions passed"
            );
        }
    }
}
