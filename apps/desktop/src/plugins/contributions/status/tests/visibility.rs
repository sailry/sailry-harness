use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{Topic, Update, plugin::ui::status::Snapshot};
use tokio::sync::mpsc;

struct Server {
    node: NodeId,
    scope: FeedScope,
    events: mpsc::UnboundedSender<bool>,
}

impl Transport for Server {
    fn target(&self) -> NodeId {
        self.node
    }

    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("visibility must not execute actions")
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert_eq!(topic, Topic::Status(self.scope));
        Box::pin(async {
            self.events.send(true).unwrap();
            Ok(Box::new(Stream {
                snapshot: Some(Snapshot {
                    node: self.node,
                    scope: self.scope,
                    entries: vec![source()],
                    failures: Vec::new(),
                }),
                events: self.events.clone(),
            }) as Box<dyn Subscription>)
        })
    }
}

struct Stream {
    snapshot: Option<Snapshot>,
    events: mpsc::UnboundedSender<bool>,
}

impl Subscription for Stream {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async {
            match self.snapshot.take() {
                Some(snapshot) => Ok(Update::Status(snapshot)),
                None => std::future::pending().await,
            }
        })
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.events.send(false);
    }
}

#[tokio::test]
async fn releases_hidden_subscription_and_retains_view() {
    let (events, mut received) = mpsc::unbounded_channel();
    let scope = FeedScope {
        worktree: Some(WorktreeId::new()),
        session: None,
    };
    let client = Arc::new(sailry_client::Client::new(Arc::new(Server {
        node: NodeId([1; 32]),
        scope,
        events,
    })));
    let (visibility, visible) = watch::channel(false);
    let (updates, mut view) = watch::channel(feed::View::default());
    let stop = CancellationToken::new();
    let observer = tokio::spawn(observe(client, scope, updates, visible, stop.clone()));
    tokio::task::yield_now().await;
    assert!(received.try_recv().is_err());
    visibility.send_replace(true);
    assert!(next(&mut received).await);
    tokio::time::timeout(std::time::Duration::from_secs(2), view.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(view.borrow().connected);
    assert_eq!(view.borrow().entries.len(), 1);
    visibility.send_replace(false);
    assert!(!next(&mut received).await);
    assert_eq!(view.borrow().entries.len(), 1);
    visibility.send_replace(true);
    assert!(next(&mut received).await);
    stop.cancel();
    assert!(!next(&mut received).await);
    observer.await.unwrap();
}

async fn next(events: &mut mpsc::UnboundedReceiver<bool>) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap()
}
