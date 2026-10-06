//! Transport acceptance with an isolated echo service, not terminal/tool acceptance.
use sailry_link::*;
use sailry_protocol::*;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "streams/admissions.rs"]
mod admissions;
#[path = "streams/subscriptions.rs"]
mod subscriptions;

#[derive(Default)]
struct Peers(Mutex<BTreeSet<NodeId>>);
impl PeerStore for Peers {
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>> {
        Box::pin(async { Ok(self.0.lock().unwrap().iter().copied().collect()) })
    }
    fn addresses(&self) -> Pending<'_, Result<Vec<EndpointAddr>, Fault>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn remember(&self, _: EndpointAddr) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async { Ok(()) })
    }
    fn set_trust(&self, peer: NodeId, trusted: bool) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async move {
            let mut peers = self.0.lock().unwrap();
            if trusted {
                peers.insert(peer);
            } else {
                peers.remove(&peer);
            }
            Ok(())
        })
    }
}

struct Echo {
    caller: NodeId,
    resource: StreamId,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    preparing: tokio::sync::Semaphore,
    cancelled: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
    completions: Mutex<Vec<tokio::sync::oneshot::Sender<Response>>>,
}
impl Handler for Echo {
    fn open(&self, caller: NodeId, resource: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        Box::pin(async move {
            if caller != self.caller {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "resource belongs to another caller",
                ));
            }
            if resource != self.resource {
                return Err(Fault::new(ErrorCode::NotFound, "resource not found"));
            }
            let (client, service) = tokio::io::duplex(1024);
            self.tasks.lock().unwrap().push(tokio::spawn(async move {
                let (mut read, mut write) = tokio::io::split(service);
                let _ = tokio::io::copy(&mut read, &mut write).await;
            }));
            Ok(Box::new(client) as Stream)
        })
    }
    fn dispatch(&self, _: NodeId, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::InstallHost { .. }) {
                let (sender, completion) = tokio::sync::oneshot::channel();
                self.completions.lock().unwrap().push(sender);
                return Ok(Admission {
                    receipt: Receipt {
                        id: request.id,
                        durable: true,
                    },
                    completion,
                });
            }
            if matches!(
                request.command,
                Command::InspectHost | Command::RegisterProject { .. }
            ) {
                let _preparing = subscriptions::Preparing(&self.cancelled);
                self.preparing.add_permits(1);
                self.release.acquire().await.unwrap().forget();
            }
            Err(Fault::new(
                ErrorCode::NotFound,
                "echo fixture has no command service",
            ))
        })
    }
    fn subscribe(
        &self,
        _: NodeId,
        topic: sailry_protocol::Topic,
    ) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            if matches!(topic, Topic::Conversation(_)) {
                self.preparing.add_permits(1);
                return Ok(Box::new(subscriptions::Idle(false)) as Box<dyn Subscription>);
            }
            if matches!(topic, Topic::Files(_)) {
                let _preparing = subscriptions::Preparing(&self.cancelled);
                self.preparing.add_permits(1);
                std::future::pending::<()>().await;
            }
            Err(Fault::new(
                ErrorCode::NotFound,
                "echo fixture has no event service",
            ))
        })
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    controller: Link,
    execution: Link,
    handler: Arc<Echo>,
    caller: NodeId,
    target: NodeId,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("controller");
        let second = directory.path().join("execution");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let controller = Identity::open(&first).unwrap();
        let execution = Identity::open(&second).unwrap();
        let caller = controller.id();
        let target = execution.id();
        let handler = Arc::new(Echo {
            caller,
            resource: StreamId::new(),
            tasks: Mutex::new(vec![]),
            preparing: tokio::sync::Semaphore::new(0),
            cancelled: tokio::sync::Semaphore::new(0),
            release: tokio::sync::Semaphore::new(0),
            completions: Mutex::new(vec![]),
        });
        let controller = Link::bind(
            &controller,
            NetworkScope::default(),
            handler.clone(),
            Arc::new(Peers::default()),
        )
        .await
        .unwrap();
        let execution = Link::bind(
            &execution,
            NetworkScope::default(),
            handler.clone(),
            Arc::new(Peers::default()),
        )
        .await
        .unwrap();
        controller.handle().set_trust(target, true).await.unwrap();
        execution.handle().set_trust(caller, true).await.unwrap();
        Self {
            _directory: directory,
            controller,
            execution,
            handler,
            caller,
            target,
        }
    }
    fn remote(&self) -> Arc<dyn Transport> {
        self.controller
            .handle()
            .remote(self.execution.handle().address())
    }
    async fn close(self) {
        self.controller.close().await.unwrap();
        self.execution.close().await.unwrap();
        let tasks = std::mem::take(&mut *self.handler.tasks.lock().unwrap());
        for task in tasks {
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap();
        }
    }
}

#[tokio::test]
async fn routing_and_half_close() {
    let fixture = Fixture::new().await;
    let local: Arc<dyn Transport> = Arc::new(Local::new(
        fixture.target,
        fixture.caller,
        fixture.handler.clone(),
    ));
    for transport in [local, fixture.remote()] {
        let mut stream = transport.open(fixture.handler.resource).await.unwrap();
        let data = vec![42; 128 * 1024];
        let (mut read, mut write) = tokio::io::split(&mut stream);
        let (_, received) = tokio::join!(
            async {
                write.write_all(&data).await.unwrap();
                write.shutdown().await.unwrap();
            },
            async {
                let mut output = Vec::new();
                read.read_to_end(&mut output).await.unwrap();
                output
            }
        );
        assert_eq!(received, data);
    }
    fixture.close().await;
}

#[tokio::test]
async fn backpressure_releases_on_shutdown() {
    let fixture = Fixture::new().await;
    let mut stream = fixture
        .remote()
        .open(fixture.handler.resource)
        .await
        .unwrap();
    // Exceeds both directions' QUIC windows and the bounded service/pump buffers.
    let data = vec![0; 32 * 1024 * 1024];
    assert!(
        tokio::time::timeout(Duration::from_millis(100), stream.write_all(&data))
            .await
            .is_err()
    );
    drop(stream);
    let mut idle = fixture
        .remote()
        .open(fixture.handler.resource)
        .await
        .unwrap();
    fixture.execution.close().await.unwrap();
    let mut byte = [0];
    assert!(
        tokio::time::timeout(Duration::from_secs(2), idle.read(&mut byte))
            .await
            .unwrap()
            .is_err()
    );
    drop(idle);
    fixture.controller.close().await.unwrap();
    let tasks = std::mem::take(&mut *fixture.handler.tasks.lock().unwrap());
    for task in tasks {
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
}

#[tokio::test]
async fn enforces_resource_authority() {
    let fixture = Fixture::new().await;
    let remote = fixture.remote();
    assert!(matches!(
        remote.open(StreamId::new()).await,
        Err(Fault {
            code: ErrorCode::NotFound,
            ..
        })
    ));
    let wrong = Local::new(fixture.target, fixture.target, fixture.handler.clone());
    assert!(matches!(
        wrong.open(fixture.handler.resource).await,
        Err(Fault {
            code: ErrorCode::PermissionDenied,
            ..
        })
    ));
    let mut stream = remote.open(fixture.handler.resource).await.unwrap();
    fixture
        .execution
        .handle()
        .set_trust(fixture.caller, false)
        .await
        .unwrap();
    let mut byte = [0];
    assert!(
        tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
            .await
            .unwrap()
            .is_err()
    );
    assert!(remote.open(fixture.handler.resource).await.is_err());
    drop(stream);
    fixture.close().await;
}

#[tokio::test]
async fn dropping_streams_releases_capacity() {
    let fixture = Fixture::new().await;
    let remote = fixture.remote();
    let mut streams = Vec::new();
    for _ in 0..4 {
        streams.push(remote.open(fixture.handler.resource).await.unwrap());
    }
    assert!(matches!(
        remote.open(fixture.handler.resource).await,
        Err(Fault {
            code: ErrorCode::Busy,
            ..
        })
    ));
    drop(streams);
    let mut restored = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match remote.open(fixture.handler.resource).await {
                Ok(stream) => break stream,
                Err(Fault {
                    code: ErrorCode::Busy,
                    ..
                }) => tokio::time::sleep(Duration::from_millis(5)).await,
                Err(error) => panic!("unexpected failure: {error}"),
            }
        }
    })
    .await
    .unwrap();
    restored.write_all(b"new stream").await.unwrap();
    let mut response = [0; 10];
    restored.read_exact(&mut response).await.unwrap();
    assert_eq!(&response, b"new stream");
    drop(restored);
    fixture.close().await;
}
