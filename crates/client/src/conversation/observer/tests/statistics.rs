use super::*;
use sailry_link::{Admission, Pending, Transport};
use sailry_protocol::conversation::{Change, Frame, Statistics};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Feed {
    node: NodeId,
    updates: watch::Sender<Update>,
    subscriptions: AtomicUsize,
}

impl Transport for Feed {
    fn target(&self) -> NodeId {
        self.node
    }

    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("statistics observation must not dispatch commands")
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert!(matches!(topic, Topic::Conversation(_)));
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
    updates: watch::Receiver<Update>,
    initial: bool,
}

impl Subscription for Stream {
    fn next(&mut self) -> sailry_link::Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            if !self.initial {
                self.updates.changed().await.unwrap();
            }
            self.initial = false;
            Ok(self.updates.borrow_and_update().clone())
        })
    }
}

async fn receive(updates: &mut watch::Receiver<View>, ready: impl Fn(&View) -> bool) -> View {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let view = updates.borrow_and_update().clone();
            assert!(view.error.is_none(), "{:#?}", view.error);
            if ready(&view) {
                return view;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .expect("statistics observation deadline")
}

#[tokio::test]
async fn keeps_messages_connected_after_failure() {
    let (snapshot, _, newer) = partial_view();
    let feed = Arc::new(Feed {
        node: snapshot.node,
        updates: watch::channel(Update::ConversationSnapshot(Box::new(snapshot.clone()))).0,
        subscriptions: AtomicUsize::new(0),
    });
    let client = Client::new(feed.clone());
    let (sender, mut updates) = watch::channel(View::default());
    let (_requests, requests) = mpsc::channel(1);
    let stop = CancellationToken::new();
    let worker = tokio::spawn({
        let stop = stop.clone();
        async move {
            client
                .watch_history(snapshot.page.session, sender, stop, requests)
                .await
        }
    });
    let pending = receive(&mut updates, |view| view.connected).await;
    assert!(pending.statistics.is_none());
    assert!(pending.statistics_error.is_none());
    let page = pending.snapshot.as_ref().unwrap().page.clone();
    let error = Fault::new(ErrorCode::Unavailable, "statistics read failed");
    feed.updates
        .send(Update::ConversationFrame(Frame {
            node: feed.node,
            session: page.session,
            sequence: 2,
            change: Change::StatisticsFailed(error.clone()),
        }))
        .unwrap();
    let failed = receive(&mut updates, |view| view.statistics_error.is_some()).await;
    assert!(failed.connected);
    assert_eq!(failed.statistics_error, Some(error));
    assert!(failed.statistics.is_none());
    assert!(Arc::ptr_eq(&failed.snapshot.as_ref().unwrap().page, &page));
    let wire = serde_json::to_value(&failed).unwrap();
    assert_eq!(wire["connected"], true);
    assert!(wire["error"].is_null());
    assert_eq!(wire["statistics_error"]["code"], "unavailable");

    let committed = entry(&newer, 8);
    feed.updates
        .send(Update::ConversationFrame(Frame {
            node: feed.node,
            session: page.session,
            sequence: 3,
            change: Change::Entry(committed.clone()),
        }))
        .unwrap();
    let messages = receive(&mut updates, |view| {
        view.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.page.entries.last() == Some(&committed))
    })
    .await;
    assert!(messages.connected);
    assert!(messages.statistics_error.is_some());
    let statistics = Statistics {
        turns: 3,
        responses: 4,
        ..Default::default()
    };
    feed.updates
        .send(Update::ConversationFrame(Frame {
            node: feed.node,
            session: page.session,
            sequence: 4,
            change: Change::Statistics(Box::new(statistics.clone())),
        }))
        .unwrap();
    let ready = receive(&mut updates, |view| view.statistics.is_some()).await;
    assert!(ready.connected);
    assert!(ready.statistics_error.is_none());
    assert_eq!(ready.statistics.as_deref(), Some(&statistics));
    assert_eq!(ready.generation, pending.generation);
    assert!(Arc::ptr_eq(
        &ready.snapshot.as_ref().unwrap().page,
        &messages.snapshot.as_ref().unwrap().page
    ));
    assert_eq!(feed.subscriptions.load(Ordering::SeqCst), 1);
    stop.cancel();
    worker.await.unwrap().unwrap();
}
