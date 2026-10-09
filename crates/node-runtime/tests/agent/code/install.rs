use super::*;
use sailry_node_runtime::CodeServerFixture;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) struct Sources {
    endpoint: String,
    requests: Arc<AtomicUsize>,
    stop: sailry_link::CancellationToken,
    task: tokio::task::JoinHandle<()>,
}
impl Sources {
    pub(super) async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let stop = sailry_link::CancellationToken::new();
        let cancelled = stop.clone();
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let fixture = CodeServerFixture::new();
        let bytes = CodeServerFixture::archive(CodeServerFixture::python());
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let asset = fixture.asset_path();
        let release = fixture.release_path();
        let metadata=serde_json::to_vec(&json!({"assets":[{"name":fixture.asset_url.rsplit('/').next().unwrap(),"digest":format!("sha256:{digest}")}]})).unwrap();
        let routes = Arc::new(BTreeMap::from([(asset, bytes), (release, metadata)]));
        let task = tokio::spawn(async move {
            loop {
                let result = tokio::select! {biased;_ = cancelled.cancelled()=>break,result=listener.accept()=>result};
                let (mut stream, _) = result.unwrap();
                let routes = routes.clone();
                let count = count.clone();
                let stop = cancelled.clone();
                tokio::spawn(async move {
                    let operation = async {
                        let mut header = Vec::new();
                        while !header.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            if stream.read_exact(&mut byte).await.is_err() || header.len() >= 8192 {
                                return;
                            }
                            header.push(byte[0]);
                        }
                        count.fetch_add(1, Ordering::SeqCst);
                        let path = String::from_utf8_lossy(&header)
                            .split_whitespace()
                            .nth(1)
                            .unwrap()
                            .to_owned();
                        let bytes = routes.get(&path);
                        let status = if bytes.is_some() {
                            "200 OK"
                        } else {
                            "404 Not Found"
                        };
                        let bytes = bytes.map(Vec::as_slice).unwrap_or(b"missing");
                        let _=stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).as_bytes()).await;
                        let _ = stream.write_all(bytes).await;
                    };
                    tokio::select! {biased;_ = stop.cancelled()=>{},_ = operation=>{}}
                });
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            task,
        }
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}

pub(super) async fn restart(
    mut fixture: Fixture,
    remote: bool,
    sources: &Sources,
    remove: Option<PathBuf>,
) -> Fixture {
    fixture.node.shutdown().await.unwrap();
    if let Some(remove) = remove {
        std::fs::remove_dir_all(remove).unwrap();
    }
    let node = Node::start_with_code(
        fixture.directory.path().join("node"),
        environment(fixture.directory.path(), &fixture.bin),
        &sources.endpoint,
    )
    .await
    .unwrap();
    let address = fixture
        .controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    fixture.client = Client::new(if remote {
        fixture.controller.handle().remote(address)
    } else {
        node.local()
    });
    fixture.node = node;
    fixture
}

#[tokio::test]
async fn requires_consent_and_recovers_original_receipts() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let sources = Sources::new().await;
        let mut fixture = Fixture::new(remote, &server).await;
        std::fs::remove_file(fixture.bin.join("rust-analyzer")).unwrap();
        fixture = restart(fixture, remote, &sources, None).await;
        let query = Query::Diagnostics {
            path: "main.rs".into(),
            severity: None,
        };
        let result = fixture.read(query.clone(), None).await;
        assert_eq!(result.status, code::Status::NotInstalled);
        assert!(!result.startup_approved);
        assert!(result.startup.is_none());
        let install = result.package_info.unwrap().install.unwrap();
        assert_eq!(sources.requests.load(Ordering::SeqCst), 0);
        let mut subscription = fixture
            .client
            .subscribe_code_servers(fixture.session.worktree)
            .await
            .unwrap();
        assert!(
            matches!(subscription.next().await.unwrap(),sailry_protocol::Update::CodeServers {installations,..} if installations.is_empty())
        );
        let request = fixture.client.prepare(Command::InstallCodeServer {
            install: install.clone(),
        });
        let admission = fixture.client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let output = admission.completion.await.unwrap().unwrap();
        let Output::CodeInstallation(state) = &output else {
            panic!("installation expected")
        };
        assert_eq!(state.status, code::InstallStatus::Ready);
        assert!(state.receipt.as_ref().unwrap().downloads[0].verified);
        assert_eq!(state.receipt.as_ref().unwrap().revision, install.revision);
        let update=tokio::time::timeout(Duration::from_secs(5),async {loop {let update=subscription.next().await.unwrap();if matches!(&update,sailry_protocol::Update::CodeServers {installations,..} if installations.iter().any(|state|state.status==code::InstallStatus::Ready)) {break update;}}}).await.unwrap();
        assert!(matches!(
            update,
            sailry_protocol::Update::CodeServers { .. }
        ));
        let cold = fixture.read(query.clone(), None).await;
        assert_eq!(cold.status, code::Status::Stopped);
        assert!(!cold.startup_approved);
        let startup = cold.startup.unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::StartCodeServer {
                startup: startup.clone(),
            }))
            .await
            .unwrap();
        let Content::Diagnostics(items) = fixture.read(query, None).await.content else {
            panic!("diagnostics expected")
        };
        assert_eq!(items.len(), 1);
        let count = sources.requests.load(Ordering::SeqCst);
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            output
        );
        assert_eq!(sources.requests.load(Ordering::SeqCst), count);
        let mut forged = request.clone();
        if let Command::InstallCodeServer { install } = &mut forged.command {
            install.revision = "0".repeat(64);
        }
        assert_eq!(
            fixture.client.execute(forged).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        let managed = fixture
            .directory
            .path()
            .join("node/tools/lsp")
            .join(&install.package)
            .join(&install.version);
        fixture = restart(fixture, remote, &sources, Some(managed)).await;
        assert_eq!(fixture.client.execute(request).await.unwrap(), output);
        assert_eq!(sources.requests.load(Ordering::SeqCst), count);
        assert!(
            !fixture
                .directory
                .path()
                .join("node/tools/lsp")
                .join(&install.package)
                .join(&install.version)
                .exists()
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn cold_read_exposes_precise_approval_after_restart() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        let query = Query::Diagnostics {
            path: "main.rs".into(),
            severity: None,
        };
        let cold = fixture.read(query.clone(), None).await;
        assert!(!cold.startup_approved);
        let startup = cold.startup.unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::StartCodeServer {
                startup: startup.clone(),
            }))
            .await
            .unwrap();
        let sources = Sources::new().await;
        fixture = restart(fixture, remote, &sources, None).await;
        let cold = fixture.read(query, None).await;
        assert_eq!(cold.status, code::Status::Stopped);
        assert_eq!(cold.startup, Some(startup));
        assert!(cold.startup_approved);
        fixture.settings(vec!["changed".into()], true).await;
        assert!(
            !fixture
                .read(
                    Query::HoverAt {
                        path: "main.rs".into(),
                        position: code::Position { line: 1, column: 3 }
                    },
                    None
                )
                .await
                .startup_approved
        );
        assert_eq!(sources.requests.load(Ordering::SeqCst), 0);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn cancels_only_the_query_owners_authenticated_request() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = Fixture::new(remote, &server).await;
        let cold = fixture
            .read(
                Query::HoverAt {
                    path: "main.rs".into(),
                    position: code::Position { line: 1, column: 3 },
                },
                None,
            )
            .await;
        let address = fixture
            .controller
            .handle()
            .pair(fixture.node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let reader = Client::new(if remote {
            fixture.controller.handle().remote(address)
        } else {
            fixture.node.local()
        });
        let foreign = Link::controller(
            fixture.directory.path().join("foreign"),
            NetworkScope::default(),
        )
        .await
        .unwrap();
        let address = foreign
            .handle()
            .pair(fixture.node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let foreign_client = Client::new(foreign.handle().remote(address));
        std::fs::write(
            fixture.root.join("control"),
            serde_json::to_vec(&json!({"hang":true})).unwrap(),
        )
        .unwrap();
        fixture
            .client
            .execute(fixture.client.prepare(Command::StartCodeServer {
                startup: cold.startup.unwrap(),
            }))
            .await
            .unwrap();
        let messages = fixture.root.join("messages");
        let offset = std::fs::read(&messages)
            .unwrap()
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        let request = reader.prepare(Command::ReadCode {
            worktree: fixture.session.worktree,
            query: Query::HoverAt {
                path: "main.rs".into(),
                position: code::Position { line: 1, column: 3 },
            },
            overlay: None,
        });
        let id = request.id;
        let mut task = tokio::spawn(async move { reader.execute(request).await });
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if std::fs::read(&messages).is_ok_and(|bytes| {
                    bytes.get(offset..).is_some_and(|bytes| {
                        bytes.split_inclusive(|byte| *byte == b'\n').any(|line| {
                            line.last() == Some(&b'\n')
                                && serde_json::from_slice::<Value>(line).is_ok_and(|message| {
                                    message.get("method").and_then(Value::as_str)
                                        == Some("textDocument/hover")
                                        && message.get("id").is_some()
                                })
                        })
                    })
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        foreign_client
            .execute(foreign_client.prepare(Command::CancelCodeQuery { request: id }))
            .await
            .unwrap();
        let unchanged = tokio::time::timeout(Duration::from_millis(50), &mut task).await;
        assert!(
            unchanged.is_err(),
            "foreign cancellation completed the query: {unchanged:?}"
        );
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::CancelCodeQuery { request: id }),
            )
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        foreign.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
