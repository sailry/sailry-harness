use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{NodeId, Request};
use std::sync::Arc;

struct Feed {
    topic: Topic,
    updates: watch::Sender<Result<Update, Fault>>,
}

impl Transport for Feed {
    fn target(&self) -> NodeId {
        NodeId([1; 32])
    }

    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("file subscriptions must not dispatch commands")
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert_eq!(topic, self.topic);
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

async fn receive(
    receiver: &mut watch::Receiver<FileView>,
    ready: impl Fn(&FileView) -> bool,
) -> FileView {
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
    .expect("file subscription deadline")
}

#[tokio::test]
async fn retains_references_across_reconnect() {
    for repository in [false, true] {
        let worktree = WorktreeId::new();
        let topic = if repository {
            Topic::Repository(worktree)
        } else {
            Topic::Files(worktree)
        };
        let snapshot = |references| {
            Ok(Update::FilesChanged {
                node: NodeId([1; 32]),
                worktree,
                references,
            })
        };
        let feed = Arc::new(Feed {
            topic,
            updates: watch::channel(snapshot(7)).0,
        });
        let client = Client::new(feed.clone());
        let (updates, mut receiver) = watch::channel(FileView::default());
        let stop = CancellationToken::new();
        let cancellation = stop.clone();
        let observer = tokio::spawn(async move {
            if repository {
                client
                    .watch_repository(worktree, updates, cancellation)
                    .await
            } else {
                client.watch_files(worktree, updates, cancellation).await
            }
        });
        let initial = receive(&mut receiver, |view| view.connected).await;
        assert_eq!(initial.references, 1);
        let _ = feed.updates.send_replace(Err(Fault::new(
            ErrorCode::Unavailable,
            "connection interrupted",
        )));
        receive(&mut receiver, |view| {
            !view.connected && view.error.is_some()
        })
        .await;
        let _ = feed.updates.send_replace(snapshot(7));
        let restored = receive(&mut receiver, |view| view.connected).await;
        assert!(restored.revision > initial.revision);
        assert_eq!(restored.references, initial.references);
        let _ = feed.updates.send_replace(snapshot(7));
        let unchanged = receive(&mut receiver, |view| view.revision > restored.revision).await;
        assert_eq!(unchanged.references, restored.references);
        let _ = feed.updates.send_replace(snapshot(8));
        let changed = receive(&mut receiver, |view| view.revision > unchanged.revision).await;
        assert_eq!(changed.references, unchanged.references + 1);
        stop.cancel();
        observer.await.unwrap().unwrap();
    }
}
