use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{Defaults, NodeId, Request, Topic, Update};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Feed {
    node: NodeId,
    changes: watch::Sender<Result<Update, Fault>>,
    subscriptions: AtomicUsize,
}

impl Feed {
    fn new() -> Arc<Self> {
        let node = NodeId([1; 32]);
        Arc::new(Self {
            node,
            changes: watch::channel(Ok(Update::Snapshot(snapshot(node)))).0,
            subscriptions: AtomicUsize::new(0),
        })
    }
}

impl Transport for Feed {
    fn target(&self) -> NodeId {
        self.node
    }
    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("Node observation must not replay commands")
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

fn snapshot(node: NodeId) -> Snapshot {
    Snapshot {
        node,
        cursor: 0,
        defaults: Defaults {
            revision: 0,
            config: None,
        },
        projects: vec![],
        worktrees: vec![],
        sessions: vec![],
        terminals: vec![],
        terminal_settings_revision: 0,
        sandbox_settings_revision: 0,
        background_settings_revision: 0,
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
    .expect("Node observer deadline")
}

fn start(
    feed: Arc<Feed>,
    observers: Arc<Observers>,
) -> (
    CancellationToken,
    watch::Receiver<View>,
    tokio::task::JoinHandle<Result<(), Fault>>,
) {
    let client = Client::new(feed).with_observers(observers);
    let stop = CancellationToken::new();
    let stopping = stop.clone();
    let (updates, receiver) = watch::channel(View::default());
    let job = tokio::spawn(async move { client.watch(updates, stopping).await });
    (stop, receiver, job)
}

#[tokio::test]
async fn shares_reducer_until_last_observer_closes() {
    let feed = Feed::new();
    let observers = Arc::new(Observers::default());
    let (first_stop, mut first, first_job) = start(feed.clone(), observers.clone());
    let (second_stop, mut second, second_job) = start(feed.clone(), observers.clone());
    receive(&mut first, |view| view.connected).await;
    receive(&mut second, |view| view.connected).await;
    assert_eq!(feed.subscriptions.load(Ordering::SeqCst), 1);
    first_stop.cancel();
    first_job.await.unwrap().unwrap();
    assert!(!first.borrow().connected);
    assert!(second.borrow().connected);
    let _ = feed
        .changes
        .send_replace(Err(Fault::new(ErrorCode::Busy, "queue full")));
    let busy = receive(&mut second, |view| {
        view.error
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::Busy)
    })
    .await;
    assert!(busy.connected);
    assert!(busy.snapshot.is_some());
    second_stop.cancel();
    second_job.await.unwrap().unwrap();
    assert!(observers.entries.lock().unwrap().is_empty());

    let _ = feed
        .changes
        .send_replace(Ok(Update::Snapshot(snapshot(feed.node))));
    let (stop, mut receiver, job) = start(feed.clone(), observers.clone());
    receive(&mut receiver, |view| view.connected).await;
    assert_eq!(feed.subscriptions.load(Ordering::SeqCst), 2);
    stop.cancel();
    job.await.unwrap().unwrap();
    assert!(observers.entries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn dropping_output_releases_the_observer() {
    let feed = Feed::new();
    let observers = Arc::new(Observers::default());
    let (_, mut receiver, job) = start(feed.clone(), observers.clone());
    receive(&mut receiver, |view| view.connected).await;
    drop(receiver);
    tokio::time::timeout(Duration::from_secs(3), job)
        .await
        .expect("observer release deadline")
        .unwrap()
        .unwrap();
    assert!(observers.entries.lock().unwrap().is_empty());
}

#[tokio::test]
async fn separates_authenticated_transports_for_same_node() {
    let first_feed = Feed::new();
    let second_feed = Feed::new();
    let observers = Arc::new(Observers::default());
    let (first_stop, mut first, first_job) = start(first_feed.clone(), observers.clone());
    let (second_stop, mut second, second_job) = start(second_feed.clone(), observers);
    receive(&mut first, |view| view.connected).await;
    receive(&mut second, |view| view.connected).await;
    assert_eq!(first_feed.subscriptions.load(Ordering::SeqCst), 1);
    assert_eq!(second_feed.subscriptions.load(Ordering::SeqCst), 1);
    let _ = first_feed
        .changes
        .send_replace(Err(Fault::new(ErrorCode::Unavailable, "transport stopped")));
    receive(&mut first, |view| !view.connected && view.error.is_some()).await;
    assert!(second.borrow().connected);
    first_stop.cancel();
    second_stop.cancel();
    first_job.await.unwrap().unwrap();
    second_job.await.unwrap().unwrap();
}

#[tokio::test]
async fn busy_does_not_claim_an_initial_connection() {
    let feed = Feed::new();
    let _ = feed
        .changes
        .send_replace(Err(Fault::new(ErrorCode::Busy, "queue full")));
    let (stop, mut receiver, job) = start(feed, Arc::new(Observers::default()));
    let view = receive(&mut receiver, |view| view.error.is_some()).await;
    assert!(!view.connected);
    assert!(view.snapshot.is_none());
    stop.cancel();
    job.await.unwrap().unwrap();
}
