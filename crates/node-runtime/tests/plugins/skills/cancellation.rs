use super::*;
use tokio::{
    io::AsyncReadExt,
    net::TcpListener,
    sync::watch,
    task::{JoinHandle, JoinSet},
};

struct Held {
    endpoint: String,
    received: watch::Receiver<usize>,
    disconnected: watch::Receiver<usize>,
    worker: JoinHandle<()>,
}

impl Held {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (received, requests) = watch::channel(0);
        let (disconnected, closed) = watch::channel(0);
        let worker = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut socket, _) = accepted.unwrap();
                        let received = received.clone();
                        let disconnected = disconnected.clone();
                        connections.spawn(async move {
                            let mut request = Vec::new();
                            let mut buffer = [0; 1024];
                            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                                let count = socket.read(&mut buffer).await.unwrap();
                                assert!(count > 0 && request.len() < 16 * 1024);
                                request.extend_from_slice(&buffer[..count]);
                            }
                            let request = String::from_utf8(request).unwrap();
                            assert!(request.starts_with(&format!("GET /owner/repo/zip/{} ", github::FIRST)));
                            received.send_modify(|count| *count += 1);
                            // Hold the response until cancellation closes the request socket.
                            assert_eq!(socket.read(&mut buffer).await.unwrap(), 0);
                            disconnected.send_modify(|count| *count += 1);
                        });
                    }
                    finished = connections.join_next(), if !connections.is_empty() => {
                        finished.unwrap().unwrap();
                    }
                }
            }
        });
        Self {
            endpoint,
            received: requests,
            disconnected: closed,
            worker,
        }
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

async fn count(receiver: &mut watch::Receiver<usize>, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while *receiver.borrow_and_update() < expected {
            receiver.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn shutdown_cancels_source_installations() {
    for remote in [false, true] {
        let mut held = Held::start().await;
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_with_skill_source(&profile, &held.endpoint)
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let source = plugin::skills::Resolved {
            repository: "https://github.com/owner/repo.git".into(),
            git_ref: "HEAD".into(),
            commit: github::FIRST.into(),
        };
        let path = "skills/analysis";
        let mut identity = blake3::Hasher::new_derive_key("Sailry standalone skill v1");
        identity.update(source.repository.as_bytes());
        identity.update(&[0]);
        identity.update(path.as_bytes());
        let skill = format!("skill-{}", &identity.finalize().to_hex()[..32]);
        let mut requests = Vec::new();
        let mut admissions = Vec::new();
        for command in [
            Command::InstallPluginSource {
                source: source.clone(),
                path: String::new(),
                name: "held-plugin".into(),
                expected_revision: 0,
            },
            Command::InstallSkill {
                source,
                path: path.into(),
                name: skill,
                expected_revision: 0,
            },
        ] {
            let request = client.prepare(command);
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            requests.push(request);
            admissions.push(admission);
        }
        count(&mut held.received, 2).await;
        for admission in &mut admissions {
            assert!(matches!(
                admission.completion.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
        }
        tokio::time::timeout(Duration::from_secs(5), node.shutdown())
            .await
            .unwrap()
            .unwrap();
        count(&mut held.disconnected, 2).await;
        drop(admissions);
        drop(client);
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert!(inventory(&client).await.is_empty());
        for request in requests {
            assert_eq!(
                client.execute(request).await.unwrap_err().code,
                ErrorCode::Cancelled
            );
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
