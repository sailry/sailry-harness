use super::*;
use sailry_link::CancellationToken;
use std::{net::SocketAddr, sync::Mutex, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const HOST: &str = "sdk.public.sailry.test";

struct Server {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    held: CancellationToken,
    disconnected: CancellationToken,
    stop: CancellationToken,
}

fn content() -> String {
    "a中文🙂".repeat(sailry_protocol::web::PAGE_CHARS / 2)
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = Self {
            address: listener.local_addr().unwrap(),
            requests: Default::default(),
            held: CancellationToken::new(),
            disconnected: CancellationToken::new(),
            stop: CancellationToken::new(),
        };
        let (requests, held, disconnected, stop) = (
            server.requests.clone(),
            server.held.clone(),
            server.disconnected.clone(),
            server.stop.clone(),
        );
        tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (mut stream, _) = tokio::select! {
                    _ = stop.cancelled() => break,
                    accepted = listener.accept() => accepted.unwrap(),
                };
                let (requests, held, disconnected) =
                    (requests.clone(), held.clone(), disconnected.clone());
                connections.spawn(async move {
                    let mut bytes = Vec::new();
                    let header = loop {
                        let mut chunk = [0; 4096];
                        let count = stream.read(&mut chunk).await.unwrap();
                        if count == 0 { return; }
                        bytes.extend_from_slice(&chunk[..count]);
                        if let Some(end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                            break String::from_utf8(bytes[..end].to_vec()).unwrap();
                        }
                    };
                    let waiting = header.starts_with("GET /hold ");
                    requests.lock().unwrap().push(header);
                    if waiting {
                        held.cancel();
                        let _ = stream.read(&mut [0; 1]).await;
                        disconnected.cancel();
                    } else {
                        let content = content();
                        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{content}",content.len());
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    }
                });
            }
            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });
        server
    }

    fn url(&self, path: &str) -> String {
        format!("http://{HOST}:{}{path}", self.address.port())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

fn install(fixture: &Fixture, granted: bool) -> Arc<Host> {
    fixture.package();
    let path = fixture.directory.path().join("project/package/plugin.json");
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["extensions"]["dev.sailry.platform"]["actions"] = if granted {
        json!(["web.read"])
    } else {
        json!([])
    };
    std::fs::write(path, manifest.to_string()).unwrap();
    let package = fixture.install(0);
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    Host::new(
        fixture.binding.client.clone(),
        Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: Some(fixture.session.worktree),
            session: Some(fixture.session.id),
        },
        fixture.runtime.handle().clone(),
        false,
        None,
    )
}

#[test]
fn reads_cached_pages_with_captured_grants() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let http = fixture.runtime.block_on(Server::start());
        fixture.node.set_web_fixture(HOST, http.address);
        let host = install(&fixture, true);
        let module = host.sdk();
        module.validate().unwrap();
        let url = http.url("/paged");
        let first = complete(&fixture, &module, "fetchWeb", json!([{"url":url}]));
        let last = complete(
            &fixture,
            &module,
            "fetchWeb",
            json!([{"url":url,"offset":first["next_offset"]}]),
        );
        assert_eq!(
            format!(
                "{}{}",
                first["content"].as_str().unwrap(),
                last["content"].as_str().unwrap()
            ),
            content()
        );
        assert_eq!(first["next_offset"], sailry_protocol::web::PAGE_CHARS);
        assert!(last["next_offset"].is_null());
        assert_eq!(first["untrusted"], true);
        assert_eq!(last["untrusted"], true);
        let context = host.context();
        let requests = fixture.transport.requests.lock().unwrap().clone();
        let fetches: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::FetchWeb(_)))
            .collect();
        assert_eq!(fetches.len(), 2);
        assert!(
            fetches
                .iter()
                .all(|request| request.plugin.as_ref() == Some(&context))
        );
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        for extra in ["headers", "credential", "method", "plugin", "worktree"] {
            assert!(
                module
                    .begin("fetchWeb", &args(json!([{"url":url,extra:"private"}])))
                    .is_err()
            );
        }
        let private = module
            .begin(
                "fetchWeb",
                &args(json!([{"url":"http://127.0.0.1/private"}])),
            )
            .unwrap();
        assert!(fixture.runtime.block_on(private).is_err());
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        let headers = http.requests.lock().unwrap()[0].to_ascii_lowercase();
        assert!(!headers.contains("authorization:"));
        assert!(!headers.contains("cookie:"));
        fixture.execute(Command::SetPluginEnabled {
            name: context.package.name.clone(),
            expected_revision: 1,
            enabled: false,
        });
        assert!(
            fixture
                .runtime
                .block_on(
                    module
                        .begin("fetchWeb", &args(json!([{"url":url}])))
                        .unwrap()
                )
                .is_err()
        );
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        host.close();
        assert!(
            module
                .begin("fetchWeb", &args(json!([{"url":url}])))
                .is_err()
        );
        fixture.close();
    }
}

#[test]
fn denies_undeclared_reads() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let http = fixture.runtime.block_on(Server::start());
        fixture.node.set_web_fixture(HOST, http.address);
        let host = install(&fixture, false);
        assert!(
            fixture
                .runtime
                .block_on(
                    host.sdk()
                        .begin("fetchWeb", &args(json!([{"url":http.url("/paged")}])))
                        .unwrap()
                )
                .is_err()
        );
        assert!(http.requests.lock().unwrap().is_empty());
        host.close();
        fixture.close();
    }
}

#[test]
fn closes_remote_reads_with_exact_cancellation() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let http = fixture.runtime.block_on(Server::start());
        fixture.node.set_web_fixture(HOST, http.address);
        let host = install(&fixture, true);
        let work = host
            .sdk()
            .begin("fetchWeb", &args(json!([{"url":http.url("/hold")}])))
            .unwrap();
        let pending = fixture.runtime.spawn(work);
        fixture.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), http.held.cancelled())
                .await
                .unwrap();
        });
        host.close();
        assert!(
            fixture
                .runtime
                .block_on(async {
                    tokio::time::timeout(Duration::from_secs(5), pending)
                        .await
                        .unwrap()
                        .unwrap()
                })
                .is_err()
        );
        fixture.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), http.disconnected.cancelled())
                .await
                .unwrap();
        });
        let requests = fixture.transport.requests.lock().unwrap();
        let fetch = requests
            .iter()
            .find(|request| matches!(request.command, Command::FetchWeb(_)))
            .unwrap();
        let cancel = requests
            .iter()
            .find(|request| matches!(request.command, Command::CancelWebFetch { .. }))
            .unwrap();
        assert!(matches!(cancel.command,Command::CancelWebFetch {request} if request == fetch.id));
        assert_eq!(cancel.plugin, fetch.plugin);
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        drop(requests);
        fixture.close();
    }
}
