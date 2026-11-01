use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    url: String,
    roots: sailry_link::CaTlsConfig,
    requests: Arc<Mutex<Vec<String>>>,
    closed: Arc<AtomicUsize>,
    release: CancellationToken,
    stop: CancellationToken,
}

impl Server {
    async fn start(status: u16, hold: bool) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (certificates, config) =
            iroh_relay::server::testing::self_signed_tls_certs_and_config();
        let roots = sailry_link::CaTlsConfig::custom_roots(certificates);
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("https://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicUsize::new(0));
        let release = CancellationToken::new();
        let stop = CancellationToken::new();
        let server = Self {
            url,
            roots,
            requests: requests.clone(),
            closed: closed.clone(),
            release: release.clone(),
            stop: stop.clone(),
        };
        tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = tasks.join_next(), if !tasks.is_empty() => {},
                    result = listener.accept() => {
                        let (stream, _) = result.unwrap();
                        let acceptor = acceptor.clone();
                        let requests = requests.clone();
                        let closed = closed.clone();
                        let release = release.clone();
                        tasks.spawn(async move {
                            let Ok(mut stream) = acceptor.accept(stream).await else { return };
                            let mut header = Vec::new();
                            let mut buffer = [0; 4096];
                            while !header.windows(4).any(|part| part == b"\r\n\r\n") {
                                let Ok(count) = stream.read(&mut buffer).await else { return };
                                if count == 0 { return; }
                                header.extend_from_slice(&buffer[..count]);
                            }
                            requests.lock().unwrap().push(String::from_utf8(header).unwrap());
                            if hold {
                                tokio::select! {
                                    _ = release.cancelled() => {},
                                    _ = stream.read(&mut buffer) => {
                                        closed.fetch_add(1, Ordering::SeqCst);
                                        return;
                                    },
                                }
                            }
                            let reply = format!("HTTP/1.1 {status} Fixture\r\nLocation: https://127.0.0.1:1/redirect\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                            let _ = stream.write_all(reply.as_bytes()).await;
                            let _ = stream.shutdown().await;
                        });
                    }
                }
            }
            tasks.shutdown().await;
        });
        server
    }

    fn selection(&self) -> Selection {
        Selection::Custom {
            relays: vec![self.url.clone()],
        }
    }

    async fn wait_requests(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.requests.lock().unwrap().len() < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("probe request deadline");
    }

    async fn wait_closed(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.closed.load(Ordering::SeqCst) < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("probe connection release deadline");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[tokio::test]
async fn coalesces_and_reuses_measurements() {
    let server = Server::start(200, true).await;
    let owner = Probes::default();
    owner.roots(server.roots.clone());
    let stop = CancellationToken::new();
    let selection = server.selection();
    let (first, second, ()) = tokio::join!(
        owner.read(&selection, &stop),
        owner.read(&selection, &stop),
        async {
            server.wait_requests(1).await;
            server.release.cancel();
        },
    );
    let first = first.unwrap();
    assert!(first.millis.is_some());
    assert_eq!(first.selection, selection);
    assert_eq!(second.unwrap(), first);
    assert_eq!(owner.read(&selection, &stop).await.unwrap(), first);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    {
        let requests = server.requests.lock().unwrap();
        assert!(requests[0].starts_with("GET /ping HTTP/1.1"));
        assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
        assert!(!requests[0].to_ascii_lowercase().contains("cookie:"));
    }
    let key = super::super::transport(&selection).urls().unwrap();
    owner
        .slot(&key)
        .unwrap()
        .lock()
        .await
        .as_mut()
        .unwrap()
        .measured -= CACHE_TTL;
    assert!(
        owner
            .read(&selection, &stop)
            .await
            .unwrap()
            .millis
            .is_some()
    );
    assert_eq!(server.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn records_only_successful_https_responses() {
    let server = Server::start(200, false).await;
    let stop = CancellationToken::new();
    assert!(
        Probes::default()
            .read(&server.selection(), &stop)
            .await
            .unwrap()
            .millis
            .is_none(),
        "untrusted certificates cannot produce a sample"
    );
    let owner = Probes::default();
    owner.roots(server.roots.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let missing = format!("https://{}", listener.local_addr().unwrap());
    drop(listener);
    let selection = Selection::Custom {
        relays: vec![missing, server.url.clone()],
    };
    assert!(
        owner
            .read(&selection, &stop)
            .await
            .unwrap()
            .millis
            .is_some()
    );
    let redirect = Server::start(302, false).await;
    owner.roots(redirect.roots.clone());
    assert!(
        owner
            .read(&redirect.selection(), &stop)
            .await
            .unwrap()
            .millis
            .is_none()
    );
    assert_eq!(
        redirect.requests.lock().unwrap().len(),
        1,
        "redirects and retries must not extend a probe"
    );
}

#[tokio::test]
async fn cancellation_releases_permits() {
    let server = Server::start(200, true).await;
    let owner = Probes::default();
    owner.roots(server.roots.clone());
    let selection = Selection::Custom {
        relays: (0..CONCURRENCY)
            .map(|index| format!("{}/relay-{index}", server.url))
            .collect(),
    };
    let stop = CancellationToken::new();
    let (result, ()) = tokio::join!(owner.read(&selection, &stop), async {
        server.wait_requests(CONCURRENCY).await;
        assert_eq!(owner.permits.available_permits(), 0);
        stop.cancel();
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    assert_eq!(owner.permits.available_permits(), CONCURRENCY);
    server.wait_closed(CONCURRENCY).await;
    server.release.cancel();
    assert!(
        owner
            .read(&selection, &CancellationToken::new())
            .await
            .unwrap()
            .millis
            .is_some()
    );
}

#[test]
fn bounds_retained_selections() {
    let owner = Probes::default();
    let key = vec!["https://relay.example/".into()];
    let first = owner.slot(&key).unwrap();
    assert!(Arc::ptr_eq(&owner.slot(&key).unwrap(), &first));
    for index in 0..CACHE_LIMIT {
        owner
            .slot(&[format!("https://relay-{index}.example/")])
            .unwrap();
    }
    assert_eq!(owner.cache.lock().unwrap().len(), CACHE_LIMIT);
    assert!(!Arc::ptr_eq(&owner.slot(&key).unwrap(), &first));
}

#[tokio::test]
async fn rejects_unsafe_selections() {
    let owner = Probes::default();
    for relays in [
        vec![],
        vec!["http://relay.example".into()],
        vec!["https://user:secret@relay.example".into()],
    ] {
        assert_eq!(
            owner
                .read(&Selection::Custom { relays }, &CancellationToken::new())
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    assert!(owner.cache.lock().unwrap().is_empty());
}
