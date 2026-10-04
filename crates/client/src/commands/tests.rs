use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{NodeId, Request};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Feed {
    node: NodeId,
    session: SessionId,
    updates: watch::Sender<Result<Update, Fault>>,
    subscriptions: AtomicUsize,
}
impl Transport for Feed {
    fn target(&self) -> NodeId {
        self.node
    }
    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("an unselected command feed must not poll requests")
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert_eq!(topic, Topic::Commands(self.session));
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
async fn receive(receiver: &mut watch::Receiver<View>, ready: impl Fn(&View) -> bool) -> View {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let view = receiver.borrow_and_update().clone();
            if ready(&view) {
                return view;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .expect("command feed deadline")
}

#[tokio::test]
async fn recovers_captured_scope() {
    let node = NodeId([1; 32]);
    let session = SessionId::new();
    let snapshot = || {
        Ok(Update::Commands {
            node,
            session,
            items: Vec::new(),
        })
    };
    let feed = Arc::new(Feed {
        node,
        session,
        updates: watch::channel(snapshot()).0,
        subscriptions: AtomicUsize::new(0),
    });
    let client = Client::new(feed.clone());
    let (_selection, selected) = watch::channel(None);
    let (updates, mut receiver) = watch::channel(View::default());
    let stop = CancellationToken::new();
    let cancellation = stop.clone();
    let observer = tokio::spawn(async move {
        client
            .watch_commands(session, selected, updates, cancellation)
            .await
    });
    receive(&mut receiver, |view| view.connected).await;
    let _ = feed.updates.send_replace(Err(Fault::new(
        ErrorCode::Unavailable,
        "connection interrupted",
    )));
    receive(&mut receiver, |view| {
        view.error
            .as_ref()
            .is_some_and(|error| error.code == ErrorCode::Unavailable)
    })
    .await;
    let _ = feed.updates.send_replace(snapshot());
    receive(&mut receiver, |view| view.connected).await;
    assert!(feed.subscriptions.load(Ordering::SeqCst) >= 2);
    for update in [
        Update::Commands {
            node: NodeId([2; 32]),
            session,
            items: Vec::new(),
        },
        Update::Commands {
            node,
            session: SessionId::new(),
            items: Vec::new(),
        },
    ] {
        let _ = feed.updates.send_replace(Ok(update));
        receive(&mut receiver, |view| {
            !view.connected
                && view
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == ErrorCode::WrongTarget)
        })
        .await;
        let _ = feed.updates.send_replace(snapshot());
        receive(&mut receiver, |view| view.connected).await;
    }
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(1), observer)
        .await
        .unwrap()
        .unwrap();
}
