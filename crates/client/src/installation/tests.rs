use super::*;
use sailry_link::{Admission, Pending, Transport};
use sailry_protocol::{
    NodeId, Request,
    installation::{Phase, Progress, Resource, State},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn queued() -> Task {
    Task {
        request: RequestId::new(),
        revision: 1,
        resource: Resource::Plugin {
            name: "fixture".into(),
        },
        state: State::Queued,
    }
}

mod projection {
    use super::*;

    #[test]
    fn retains_snapshot_order() {
        let first = queued();
        let second = queued();
        let mut projection = Projection::default();
        assert_eq!(
            projection
                .apply(vec![second.clone(), first.clone()])
                .unwrap(),
            vec![second, first]
        );
    }

    #[test]
    fn ignores_older_task_revisions() {
        let before = queued();
        let mut current = before.clone();
        current.revision = 2;
        current.state = State::Running {
            phase: Phase::Inspecting,
            progress: None,
        };
        let mut projection = Projection::default();
        projection.apply(vec![current.clone()]).unwrap();
        assert_eq!(projection.apply(vec![before]).unwrap(), vec![current]);
    }

    #[test]
    fn does_not_resurrect_removed_rows() {
        let task = queued();
        let mut projection = Projection::default();
        projection.apply(vec![task.clone()]).unwrap();
        assert!(projection.apply(Vec::new()).unwrap().is_empty());
        assert!(projection.apply(vec![task]).unwrap().is_empty());
    }

    #[test]
    fn validates_the_whole_snapshot() {
        let task = queued();
        let mut projection = Projection::default();
        projection.apply(vec![task.clone()]).unwrap();
        assert!(projection.apply(vec![task.clone(), task.clone()]).is_err());
        let mut invalid = task.clone();
        invalid.revision = 2;
        invalid.state = State::Running {
            phase: Phase::Downloading,
            progress: Some(Progress::Counts {
                completed: 0,
                total: 0,
                bytes_per_second: None,
            }),
        };
        assert!(projection.apply(vec![invalid]).is_err());
        assert_eq!(projection.tasks[&task.request], task);
    }
}

struct Feed {
    node: NodeId,
    updates: watch::Sender<Result<Update, Fault>>,
    subscriptions: AtomicUsize,
}

impl Transport for Feed {
    fn target(&self) -> NodeId {
        self.node
    }
    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("installation observation must not execute requests")
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert_eq!(topic, Topic::Installations);
        self.subscriptions.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(Box::new(Stream {
                updates: self.updates.subscribe(),
                initial: true,
            }) as Box<dyn Subscription>)
        })
    }
}

struct Stream {
    updates: watch::Receiver<Result<Update, Fault>>,
    initial: bool,
}
impl Subscription for Stream {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if !self.initial {
                self.updates.changed().await.unwrap();
            }
            self.initial = false;
            self.updates.borrow_and_update().clone()
        })
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
    .expect("installation observer deadline")
}

#[tokio::test]
async fn reconnects_without_replaying_requests() {
    let node = NodeId([1; 32]);
    let mut task = queued();
    task.state = State::Failed {
        error: Fault::invalid("fixture failure"),
    };
    let feed = Arc::new(Feed {
        node,
        updates: watch::channel(Ok(Update::Installations {
            node,
            tasks: vec![task.clone()],
        }))
        .0,
        subscriptions: AtomicUsize::new(0),
    });
    let client = Client::new(feed.clone());
    let (updates, mut receiver) = watch::channel(View::default());
    let stop = CancellationToken::new();
    let stopping = stop.clone();
    let observer = tokio::spawn(async move { client.watch_installations(updates, stopping).await });
    receive(&mut receiver, |view| view.connected).await;
    let _ = feed
        .updates
        .send_replace(Err(Fault::new(ErrorCode::Unavailable, "disconnected")));
    let offline = receive(&mut receiver, |view| view.error.is_some()).await;
    assert_eq!(offline.request(task.request), Some(&task));
    let _ = feed.updates.send_replace(Ok(Update::Installations {
        node,
        tasks: Vec::new(),
    }));
    receive(&mut receiver, |view| {
        view.connected && view.tasks.is_empty()
    })
    .await;
    assert!(feed.subscriptions.load(Ordering::SeqCst) >= 2);
    stop.cancel();
    observer.await.unwrap().unwrap();
}

#[tokio::test]
async fn rejects_another_nodes_snapshot() {
    let node = NodeId([1; 32]);
    let feed = Arc::new(Feed {
        node,
        updates: watch::channel(Ok(Update::Installations {
            node: NodeId([2; 32]),
            tasks: vec![queued()],
        }))
        .0,
        subscriptions: AtomicUsize::new(0),
    });
    let client = Client::new(feed);
    let (updates, receiver) = watch::channel(View::default());
    let result = client
        .watch_installations(updates, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(result.code, ErrorCode::WrongTarget);
    assert!(receiver.borrow().tasks.is_empty());
}

struct ScopedFeed {
    feed: Feed,
    request: Request,
}

impl Transport for ScopedFeed {
    fn target(&self) -> NodeId {
        self.feed.node
    }
    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("scoped observation must not execute requests")
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert_eq!(topic, Topic::ScopedInstallations(self.request.clone()));
        self.feed.subscribe(Topic::Installations)
    }
}

#[tokio::test]
async fn reconnects_with_original_plugin_scope() {
    use sailry_protocol::plugin::{Context, Reference};
    let node = NodeId([1; 32]);
    let task = queued();
    let request = Request::new(
        node,
        Command::ReadInstallations {
            requests: vec![task.request],
        },
    )
    .with_plugin(Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: Reference {
            name: "fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 1,
        },
        worktree: None,
        session: None,
    });
    let feed = Arc::new(ScopedFeed {
        feed: Feed {
            node,
            updates: watch::channel(Ok(Update::Installations {
                node,
                tasks: vec![task.clone()],
            }))
            .0,
            subscriptions: AtomicUsize::new(0),
        },
        request: request.clone(),
    });
    let client = Client::new(feed.clone());
    let (updates, mut receiver) = watch::channel(View::default());
    let stop = CancellationToken::new();
    let stopping = stop.clone();
    let observer = tokio::spawn(async move {
        client
            .watch_installations_for(request, updates, stopping)
            .await
    });
    receive(&mut receiver, |view| view.connected).await;
    let _ = feed
        .feed
        .updates
        .send_replace(Err(Fault::new(ErrorCode::Unavailable, "disconnected")));
    receive(&mut receiver, |view| view.error.is_some()).await;
    let _ = feed.feed.updates.send_replace(Ok(Update::Installations {
        node,
        tasks: Vec::new(),
    }));
    receive(&mut receiver, |view| {
        view.connected && view.tasks.is_empty()
    })
    .await;
    assert!(feed.feed.subscriptions.load(Ordering::SeqCst) >= 2);
    stop.cancel();
    observer.await.unwrap().unwrap();
}
