use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{NodeId, Request, python::Stage};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

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
        panic!("preparation observation must not execute requests")
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert_eq!(topic, Topic::Python);
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
    .expect("preparation observer deadline")
}

fn installing(node: NodeId) -> Update {
    Update::Python {
        node,
        preparations: vec![Preparation {
            package: "office".into(),
            stage: Stage::Installing,
        }],
    }
}

#[tokio::test]
async fn reconnects_without_replaying_preparation() {
    let node = NodeId([1; 32]);
    let feed = Arc::new(Feed {
        node,
        updates: watch::channel(Ok(installing(node))).0,
        subscriptions: AtomicUsize::new(0),
    });
    let client = Client::new(feed.clone());
    let (updates, mut receiver) = watch::channel(View::default());
    let stop = CancellationToken::new();
    let cancellation = stop.clone();
    let observer = tokio::spawn(async move { client.watch_python(updates, cancellation).await });
    let active = receive(&mut receiver, |view| view.package("office").is_some()).await;
    assert!(active.package("other").is_none());
    let _ = feed
        .updates
        .send_replace(Err(Fault::new(ErrorCode::Unavailable, "disconnected")));
    let offline = receive(&mut receiver, |view| view.error.is_some()).await;
    assert!(!offline.connected);
    assert!(offline.preparations.is_empty());
    let _ = feed.updates.send_replace(Ok(installing(node)));
    receive(&mut receiver, |view| view.package("office").is_some()).await;
    assert!(feed.subscriptions.load(Ordering::SeqCst) >= 2);
    let _ = feed.updates.send_replace(Ok(Update::Python {
        node,
        preparations: Vec::new(),
    }));
    receive(&mut receiver, |view| {
        view.connected && view.preparations.is_empty()
    })
    .await;
    stop.cancel();
    observer.await.unwrap().unwrap();
    assert!(!receiver.borrow().connected);
    assert!(receiver.borrow().preparations.is_empty());
}

#[tokio::test]
async fn rejects_foreign_nodes() {
    let node = NodeId([1; 32]);
    let feed = Arc::new(Feed {
        node,
        updates: watch::channel(Ok(installing(NodeId([2; 32])))).0,
        subscriptions: AtomicUsize::new(0),
    });
    let client = Client::new(feed);
    let (updates, receiver) = watch::channel(View::default());
    let error = client
        .watch_python(updates, CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::WrongTarget);
    assert!(receiver.borrow().preparations.is_empty());
    assert!(!receiver.borrow().connected);
}
