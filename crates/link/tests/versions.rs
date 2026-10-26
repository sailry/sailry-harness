use sailry_link::*;
use sailry_protocol::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Service {
    versions: Vec<Option<String>>,
    active: AtomicUsize,
    calls: AtomicUsize,
}

impl Service {
    fn new() -> Self {
        let version = semver::Version::parse(PRODUCT_VERSION).unwrap();
        let mut metadata = version.clone();
        metadata.build = semver::BuildMetadata::new("fixture").unwrap();
        Self {
            versions: vec![
                Some(PRODUCT_VERSION.into()),
                Some("0.0.0".into()),
                Some("99.0.0".into()),
                Some("invalid".into()),
                None,
                Some(metadata.to_string()),
            ],
            active: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
        }
    }

    fn reached<T>(&self) -> Result<T, Fault> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(Fault::new(ErrorCode::NotFound, "service reached"))
    }
}

impl Handler for Service {
    fn product_version(&self) -> Option<&str> {
        self.versions[self.active.load(Ordering::SeqCst)].as_deref()
    }
    fn open(&self, _: NodeId, _: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        Box::pin(async { self.reached() })
    }
    fn dispatch(&self, _: NodeId, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async { self.reached() })
    }
    fn subscribe(&self, _: NodeId, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async { self.reached() })
    }
}

async fn faults(transport: &dyn Transport) -> Vec<Fault> {
    let request = Request::new(transport.target(), Command::Snapshot);
    let command = transport.dispatch(request).await.err().unwrap();
    let stream = transport.open(StreamId::new()).await.err().unwrap();
    let subscription = transport.subscribe(Topic::Node).await.err().unwrap();
    vec![command, stream, subscription]
}

fn denied(faults: Vec<Fault>, update_host: bool) {
    for fault in faults {
        assert_eq!(fault.code, ErrorCode::Conflict);
        assert!(matches!(fault.details, Some(FaultDetails::VersionMismatch {
            update_host: actual, ..
        }) if actual == update_host));
    }
}

#[tokio::test]
async fn local_blocks_incompatible_business() {
    let service = Arc::new(Service::new());
    let transport = Local::new(NodeId([1; 32]), NodeId([2; 32]), service.clone());
    for active in [0, 5] {
        service.active.store(active, Ordering::SeqCst);
        assert!(
            faults(&transport)
                .await
                .iter()
                .all(|fault| fault.code == ErrorCode::NotFound)
        );
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 6);
    for (active, update_host) in [(1, true), (2, false), (3, true), (4, true)] {
        service.active.store(active, Ordering::SeqCst);
        denied(faults(&transport).await, update_host);
    }
    assert_eq!(service.calls.load(Ordering::SeqCst), 6);
}

#[derive(Default)]
struct Peers;
impl PeerStore for Peers {
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn addresses(&self) -> Pending<'_, Result<Vec<EndpointAddr>, Fault>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn remember(&self, _: EndpointAddr) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async { Ok(()) })
    }
    fn set_trust(&self, _: NodeId, _: bool) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn remote_checks_each_authenticated_call() {
    let directory = tempfile::tempdir().unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let identity = Identity::open(directory.path()).unwrap();
    let service = Arc::new(Service::new());
    let host = Link::bind(
        &identity,
        NetworkScope::default(),
        service.clone(),
        Arc::new(Peers),
    )
    .await
    .unwrap();
    controller
        .handle()
        .set_trust(identity.id(), true)
        .await
        .unwrap();
    host.handle()
        .set_trust(NodeId(*controller.handle().address().id.as_bytes()), true)
        .await
        .unwrap();
    let address = host.handle().address();
    let transport = controller.handle().remote(address.clone());
    assert!(
        controller
            .handle()
            .inspect(&address)
            .await
            .unwrap()
            .compatibility()
            .is_ok()
    );
    assert!(
        faults(&*transport)
            .await
            .iter()
            .all(|fault| fault.code == ErrorCode::NotFound)
    );
    assert_eq!(service.calls.load(Ordering::SeqCst), 3);
    // A retained connection cannot bypass the receiver's current version check.
    for (active, update_host) in [(1, true), (2, false), (3, true), (4, true)] {
        service.active.store(active, Ordering::SeqCst);
        denied(faults(&*transport).await, update_host);
        assert_eq!(service.calls.load(Ordering::SeqCst), 3);
        controller.handle().disconnect(identity.id()).await;
        let info = controller.handle().inspect(&address).await.unwrap();
        assert!(info.compatibility().is_err());
        denied(faults(&*transport).await, update_host);
        controller.handle().disconnect(identity.id()).await;
    }
    service.active.store(5, Ordering::SeqCst);
    controller.handle().disconnect(identity.id()).await;
    assert!(
        faults(&*transport)
            .await
            .iter()
            .all(|fault| fault.code == ErrorCode::NotFound)
    );
    assert_eq!(service.calls.load(Ordering::SeqCst), 6);
    controller.close().await.unwrap();
    host.close().await.unwrap();
}
