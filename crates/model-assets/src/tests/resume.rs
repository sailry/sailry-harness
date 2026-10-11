use super::*;
use std::collections::VecDeque;

struct Response {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

impl Response {
    fn full(body: &[u8]) -> Self {
        Self {
            status: 200,
            headers: format!("Content-Length: {}\r\nETag: \"pinned\"\r\n", body.len()),
            body: body.into(),
        }
    }
    fn interrupted() -> Self {
        Self {
            status: 200,
            headers: "Content-Length: 6\r\nETag: \"pinned\"\r\n".into(),
            body: b"abc".into(),
        }
    }
    fn range() -> Self {
        Self {
            status: 206,
            headers: "Content-Length: 3\r\nETag: \"pinned\"\r\nContent-Range: bytes 3-5/6\r\n"
                .into(),
            body: b"def".into(),
        }
    }
}

struct Endpoint {
    url: Url,
    requests: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Endpoint {
    async fn start(responses: Vec<Response>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!(
            "http://{}/model?token=private-secret",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let mut responses = VecDeque::from(responses);
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if socket.read(&mut byte).await.unwrap() == 0 {
                        break;
                    }
                    request.extend(byte);
                    assert!(request.len() < 8192);
                }
                captured
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap().to_ascii_lowercase());
                let response = responses.pop_front().expect("unexpected download request");
                let header = format!(
                    "HTTP/1.1 {} Fixture\r\n{}Connection: close\r\n\r\n",
                    response.status, response.headers
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                let _ = socket.write_all(&response.body).await;
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }

    async fn acquire(
        &self,
        path: &Path,
        cancel: &CancellationToken,
        progress: impl FnMut(Progress),
    ) -> Result<std::path::PathBuf> {
        let hash = checksum(b"abcdef");
        transfer::acquire(
            path,
            &[Download {
                file: File {
                    name: "model",
                    size: 6,
                    sha256: &hash,
                },
                url: self.url.clone(),
            }],
            cancel,
            progress,
            &client(Duration::from_secs(1)),
        )
        .await
    }

    async fn pause(&self, path: &Path) {
        let cancel = CancellationToken::new();
        assert_eq!(
            self.acquire(path, &cancel, |progress| {
                if progress.bytes_per_second > 0. {
                    cancel.cancel();
                }
            })
            .await,
            Err(Error::Cancelled)
        );
        let part = partial(path);
        assert_eq!(std::fs::read(part).unwrap(), b"abc");
        let marker = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .unwrap();
        let checkpoint = std::fs::read_to_string(marker).unwrap();
        assert!(!checkpoint.contains("private-secret"));
        assert!(!checkpoint.contains(self.url.as_str()));
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn partial(path: &Path) -> std::path::PathBuf {
    std::fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "part")
        })
        .unwrap()
}

#[tokio::test]
async fn preserves_verified_prefix_after_cancellation() {
    let root = directory();
    let endpoint = Endpoint::start(vec![Response::interrupted(), Response::range()]).await;
    endpoint.pause(root.path()).await;
    let mut values = Vec::new();
    endpoint
        .acquire(root.path(), &CancellationToken::new(), |value| {
            values.push(value)
        })
        .await
        .unwrap();
    let requests = endpoint.requests.lock().unwrap();
    assert!(!requests[0].contains("range:"));
    assert!(requests[1].contains("range: bytes=3-\r\n"));
    assert!(requests[1].contains("if-range: \"pinned\"\r\n"));
    assert!(
        requests
            .iter()
            .all(|request| request.contains("accept-encoding: identity\r\n"))
    );
    assert_eq!(values[0].percent, 50.);
    assert_eq!(values[0].bytes_per_second, 0.);
    assert_eq!(values.last().unwrap().percent, 100.);
    assert_eq!(std::fs::read(root.path().join("model")).unwrap(), b"abcdef");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn accepts_full_response_to_range() {
    let root = directory();
    let endpoint = Endpoint::start(vec![Response::interrupted(), Response::full(b"abcdef")]).await;
    endpoint.pause(root.path()).await;
    endpoint
        .acquire(root.path(), &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(root.path().join("model")).unwrap(), b"abcdef");
    assert_eq!(endpoint.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn restarts_invalid_range_or_validator_once() {
    for headers in [
        "Content-Length: 3\r\nETag: \"pinned\"\r\nContent-Range: bytes 2-4/6\r\n",
        "Content-Length: 3\r\nETag: \"changed\"\r\nContent-Range: bytes 3-5/6\r\n",
    ] {
        let root = directory();
        let endpoint = Endpoint::start(vec![
            Response::interrupted(),
            Response {
                status: 206,
                headers: headers.into(),
                body: b"def".into(),
            },
            Response::full(b"abcdef"),
        ])
        .await;
        endpoint.pause(root.path()).await;
        endpoint
            .acquire(root.path(), &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        let requests = endpoint.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[1].contains("range: bytes=3-"));
        assert!(!requests[2].contains("range:"));
        assert_eq!(std::fs::read(root.path().join("model")).unwrap(), b"abcdef");
    }
}

#[tokio::test]
async fn restarts_unsatisfied_range_with_exact_size() {
    let root = directory();
    let endpoint = Endpoint::start(vec![
        Response::interrupted(),
        Response {
            status: 416,
            headers: "Content-Length: 0\r\nContent-Range: bytes */6\r\n".into(),
            body: vec![],
        },
        Response::full(b"abcdef"),
    ])
    .await;
    endpoint.pause(root.path()).await;
    endpoint
        .acquire(root.path(), &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(root.path().join("model")).unwrap(), b"abcdef");
    assert!(!endpoint.requests.lock().unwrap()[2].contains("range:"));
}

#[tokio::test]
async fn rejects_inconsistent_unsatisfied_size() {
    let root = directory();
    std::fs::write(root.path().join("model"), b"previous").unwrap();
    let endpoint = Endpoint::start(vec![
        Response::interrupted(),
        Response {
            status: 416,
            headers: "Content-Length: 0\r\nContent-Range: bytes */99\r\n".into(),
            body: vec![],
        },
    ])
    .await;
    endpoint.pause(root.path()).await;
    assert_eq!(
        endpoint
            .acquire(root.path(), &CancellationToken::new(), |_| {})
            .await,
        Err(Error::InvalidAsset)
    );
    assert_eq!(
        std::fs::read(root.path().join("model")).unwrap(),
        b"previous"
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn restarts_damaged_prefix_or_checkpoint() {
    for marker in [false, true] {
        let root = directory();
        let endpoint =
            Endpoint::start(vec![Response::interrupted(), Response::full(b"abcdef")]).await;
        endpoint.pause(root.path()).await;
        let path = if marker {
            partial(root.path()).with_extension("part.json")
        } else {
            partial(root.path())
        };
        std::fs::write(path, b"bad").unwrap();
        endpoint
            .acquire(root.path(), &CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert!(!endpoint.requests.lock().unwrap()[1].contains("range:"));
        assert_eq!(std::fs::read(root.path().join("model")).unwrap(), b"abcdef");
    }
}

#[tokio::test]
async fn preserves_prefix_after_connection_failure() {
    let root = directory();
    let endpoint = Endpoint::start(vec![Response::interrupted(), Response::range()]).await;
    let Error::Download(reason) = endpoint
        .acquire(root.path(), &CancellationToken::new(), |_| {})
        .await
        .unwrap_err()
    else {
        panic!("download failure expected")
    };
    assert!(
        reason.contains("body") || reason.contains("connection"),
        "{reason}"
    );
    assert_eq!(std::fs::read(partial(root.path())).unwrap(), b"abc");
    endpoint
        .acquire(root.path(), &CancellationToken::new(), |_| {})
        .await
        .unwrap();
    assert!(endpoint.requests.lock().unwrap()[1].contains("range: bytes=3-"));
}

#[tokio::test]
async fn concurrent_downloads_share_one_transfer() {
    let root = directory();
    let endpoint = Endpoint::start(vec![Response::full(b"abcdef")]).await;
    let first = CancellationToken::new();
    let second = CancellationToken::new();
    let (first, second) = tokio::join!(
        endpoint.acquire(root.path(), &first, |_| {}),
        endpoint.acquire(root.path(), &second, |_| {})
    );
    first.unwrap();
    second.unwrap();
    assert_eq!(endpoint.requests.lock().unwrap().len(), 1);
    assert_eq!(std::fs::read(root.path().join("model")).unwrap(), b"abcdef");
}
