use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tracing::instrument::WithSubscriber as _;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Capture {
    fn dispatch(&self) -> tracing::Dispatch {
        tracing::Dispatch::new(
            tracing_subscriber::fmt()
                .json()
                .without_time()
                .with_writer(self.clone())
                .finish(),
        )
    }
    fn records(&self) -> Vec<serde_json::Value> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl std::io::Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

struct Feed {
    node: NodeId,
    changes: watch::Sender<Result<Update, Fault>>,
    reads: Mutex<Vec<RequestId>>,
    subscriptions: AtomicUsize,
}
impl Feed {
    fn new() -> Arc<Self> {
        let node = NodeId([1; 32]);
        Arc::new(Self {
            node,
            changes: watch::channel(Ok(Update::Snapshot(snapshot(node, 0)))).0,
            reads: Mutex::new(Vec::new()),
            subscriptions: AtomicUsize::new(0),
        })
    }
}
impl Transport for Feed {
    fn target(&self) -> NodeId {
        self.node
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        assert!(
            matches!(request.command, Command::Snapshot),
            "recovery must never replay a mutation"
        );
        self.reads.lock().unwrap().push(request.id);
        Box::pin(async move {
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender
                .send(Ok(Output::Snapshot(snapshot(self.node, 20))))
                .unwrap();
            Ok(Admission {
                receipt: Receipt {
                    id: request.id,
                    durable: false,
                },
                completion,
            })
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert_eq!(topic, Topic::Node);
        self.subscriptions.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(Box::new(Stream {
                receiver: self.changes.subscribe(),
                initial: true,
            }) as Box<dyn Subscription>)
        })
    }
}
struct Stream {
    receiver: watch::Receiver<Result<Update, Fault>>,
    initial: bool,
}
impl Subscription for Stream {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if !self.initial {
                self.receiver.changed().await.unwrap();
            }
            self.initial = false;
            self.receiver.borrow_and_update().clone()
        })
    }
}

fn snapshot(node: NodeId, cursor: u64) -> Snapshot {
    Snapshot {
        node,
        cursor,
        defaults: Defaults {
            revision: 0,
            config: None,
        },
        projects: vec![],
        worktrees: vec![],
        sessions: vec![],
        terminals: vec![],
        terminal_settings_revision: 0,
        power_settings_revision: 0,
        sandbox_settings_revision: 0,
        search_settings_revision: 0,
        media_settings: Default::default(),
        providers: vec![],
        provider_quotas: vec![],
        model_catalog: Default::default(),
        roles: vec![],
        ssh: vec![],
        databases: vec![],
        plugins: vec![],
        notifications: vec![],
        turns: vec![],
    }
}

async fn receive(receiver: &mut watch::Receiver<View>, predicate: impl Fn(&View) -> bool) -> View {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let view = receiver.borrow_and_update().clone();
            if predicate(&view) {
                return view;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .expect("observation deadline")
}

#[tokio::test]
async fn correlates_recovery_without_logging_events() {
    let feed = Feed::new();
    let client = Client::new(feed.clone());
    let captured = Capture::default();
    let stop = CancellationToken::new();
    let (sender, mut receiver) = watch::channel(View::default());
    let observation = client
        .watch(sender, stop.clone())
        .with_subscriber(captured.dispatch());
    let driver = async {
        receive(&mut receiver, |view| view.connected).await;
        for cursor in 1..=16 {
            let _ = feed.changes.send_replace(Ok(Update::Event(EventEnvelope {
                node: feed.node,
                cursor,
                event: Event::DefaultsChanged(Defaults {
                    revision: cursor,
                    config: None,
                }),
            })));
            receive(&mut receiver, |view| {
                view.snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.cursor == cursor)
            })
            .await;
        }
        let _ = feed.changes.send_replace(Ok(Update::ResetRequired));
        receive(&mut receiver, |view| {
            view.snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.cursor == 20)
        })
        .await;
        let _ = feed.changes.send_replace(Err(Fault::new(
            ErrorCode::Unavailable,
            "PRIVATE_CONNECTION_ERROR",
        )));
        receive(&mut receiver, |view| view.error.is_some()).await;
        let _ = feed
            .changes
            .send_replace(Ok(Update::Snapshot(snapshot(feed.node, 30))));
        receive(&mut receiver, |view| {
            view.connected
                && view
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.cursor == 30)
        })
        .await;
        stop.cancel();
    };
    let (result, _) = tokio::join!(observation, driver);
    result.unwrap();
    assert_eq!(feed.subscriptions.load(Ordering::SeqCst), 2);
    let reads = feed.reads.lock().unwrap();
    assert_eq!(reads.len(), 1);
    let records = captured.records();
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );
    let lifecycle: Vec<_> = records
        .iter()
        .filter(|record| record["fields"]["operation"] == "node_subscription")
        .map(|record| &record["fields"])
        .collect();
    assert_eq!(lifecycle.first().unwrap()["phase"], "begin");
    assert_eq!(lifecycle.last().unwrap()["phase"], "completed");
    let calls: std::collections::BTreeSet<_> = lifecycle
        .iter()
        .map(|fields| fields["call"].as_str().unwrap())
        .collect();
    assert_eq!(calls.len(), 1);
    assert!(
        lifecycle
            .iter()
            .all(|fields| fields["node"] == feed.node.to_string())
    );
    let ready: Vec<_> = lifecycle
        .iter()
        .filter(|fields| fields["phase"] == "ready")
        .map(|fields| fields["generation"].as_u64().unwrap())
        .collect();
    assert_eq!(ready, [1, 2]);
    assert!(
        lifecycle
            .iter()
            .any(|fields| fields["phase"] == "reconnecting" && fields["generation"] == 2)
    );
    assert!(
        lifecycle
            .iter()
            .any(|fields| fields["recovery_decision"] == "reconnect" && fields["wait_ms"] == 250)
    );
    let recovery: Vec<_> = records
        .iter()
        .filter(|record| record["fields"]["operation"] == "snapshot_recovery")
        .map(|record| &record["fields"])
        .collect();
    assert_eq!(recovery.len(), 2);
    assert_eq!(recovery[0]["phase"], "begin");
    assert_eq!(recovery[1]["phase"], "completed");
    assert!(
        recovery
            .iter()
            .all(|fields| fields["call"] == lifecycle[0]["call"])
    );
    assert!(
        recovery
            .iter()
            .all(|fields| fields["request"] == reads[0].to_string())
    );
    assert!(
        records.len() <= 10,
        "ordinary events must not produce diagnostic records"
    );
}

#[tokio::test]
async fn stops_after_authorization_failure() {
    let feed = Feed::new();
    let _ = feed.changes.send_replace(Err(Fault::new(
        ErrorCode::PermissionDenied,
        "PRIVATE_AUTH_ERROR",
    )));
    let client = Client::new(feed.clone());
    let captured = Capture::default();
    let (sender, _receiver) = watch::channel(View::default());
    let error = client
        .watch(sender, CancellationToken::new())
        .with_subscriber(captured.dispatch())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(feed.subscriptions.load(Ordering::SeqCst), 1);
    assert!(feed.reads.lock().unwrap().is_empty());
    let records = captured.records();
    assert_eq!(records.last().unwrap()["fields"]["phase"], "failed");
    assert_eq!(
        records.last().unwrap()["fields"]["code"],
        "permission_denied"
    );
    assert!(
        records
            .iter()
            .filter(|record| record["fields"]["phase"] != "begin")
            .all(|record| record["fields"]["recovery_decision"] == "stop")
    );
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );
}
