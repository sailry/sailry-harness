use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{NodeId, SessionId, WorktreeId, plugin::ui::Slot};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Feed {
    node: NodeId,
    scope: Scope,
    updates: watch::Sender<Result<Update, Fault>>,
    subscriptions: AtomicUsize,
}

impl Feed {
    fn new(entries: Vec<Entry>, failures: Vec<Failure>) -> Arc<Self> {
        let node = NodeId([1; 32]);
        let scope = Scope {
            worktree: Some(WorktreeId::new()),
            session: Some(SessionId::new()),
        };
        Arc::new(Self {
            node,
            scope,
            updates: watch::channel(Ok(Update::Status(status::Snapshot {
                node,
                scope,
                entries,
                failures,
            })))
            .0,
            subscriptions: AtomicUsize::new(0),
        })
    }
}

impl Transport for Feed {
    fn target(&self) -> NodeId {
        self.node
    }

    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("a status observer must not execute an action")
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        assert_eq!(topic, Topic::Status(self.scope));
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

fn entry(package: &str, id: &str, order: i16) -> Entry {
    let declaration = serde_json::from_value(serde_json::json!({
        "id": id, "slot": "status", "kind": "button",
        "label": {"label": "Badge"}, "handler": "desktop", "order": order
    }))
    .unwrap();
    let mut state = sailry_protocol::plugin::ui::State::pending(&declaration);
    state.visible = true;
    state.enabled = true;
    Entry {
        package: sailry_protocol::plugin::Reference {
            name: package.into(),
            digest: "captured".into(),
            settings_revision: 3,
        },
        declaration,
        state,
        invoke: Some("activate".into()),
    }
}

fn observe(
    feed: Arc<Feed>,
) -> (
    watch::Receiver<View>,
    CancellationToken,
    tokio::task::JoinHandle<()>,
) {
    let client = Client::new(feed.clone());
    let (updates, receiver) = watch::channel(View::default());
    let stop = CancellationToken::new();
    let cancellation = stop.clone();
    let observer = tokio::spawn(async move {
        client.watch_status(feed.scope, updates, cancellation).await;
    });
    (receiver, stop, observer)
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
    .expect("status feed deadline")
}

async fn close(stop: CancellationToken, observer: tokio::task::JoinHandle<()>) {
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(1), observer)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn reduces_and_orders_entries() {
    let first = entry("z", "first", -1);
    let second = entry("a", "a", 0);
    let third = entry("a", "b", 0);
    let fourth = entry("z", "a", 0);
    let mut hidden = entry("a", "hidden", 1);
    hidden.state.visible = false;
    hidden.state.enabled = false;
    let mut invalid = entry("a", "invalid", -2);
    invalid.state.reply_to = Some(1);
    let mut misplaced = entry("a", "misplaced", -2);
    misplaced.declaration.slot = Slot::Composer;
    let failures = vec![Failure {
        package: first.package.clone(),
        fault: Fault::new(ErrorCode::Unavailable, "read interrupted"),
    }];
    let feed = Feed::new(
        vec![
            fourth.clone(),
            hidden.clone(),
            third.clone(),
            invalid,
            second.clone(),
            first.clone(),
            misplaced,
        ],
        failures.clone(),
    );
    let (mut receiver, stop, observer) = observe(feed);
    let view = receive(&mut receiver, |view| view.connected).await;
    assert_eq!(view.entries, vec![first, second, third, fourth, hidden]);
    assert_eq!(view.failures, failures);
    assert!(view.error.is_none());
    close(stop, observer).await;
}

#[tokio::test]
async fn reconnect_retains_last_state() {
    let expected = entry("fixture", "badge", 0);
    let feed = Feed::new(vec![expected.clone()], vec![]);
    let (mut receiver, stop, observer) = observe(feed.clone());
    receive(&mut receiver, |view| view.connected).await;
    let _ = feed.updates.send_replace(Err(Fault::new(
        ErrorCode::Unavailable,
        "connection interrupted",
    )));
    let disconnected = receive(&mut receiver, |view| {
        !view.connected
            && view
                .error
                .as_ref()
                .is_some_and(|fault| fault.code == ErrorCode::Unavailable)
    })
    .await;
    assert_eq!(disconnected.entries, vec![expected.clone()]);
    let _ = feed
        .updates
        .send_replace(Ok(Update::Status(status::Snapshot {
            node: feed.node,
            scope: feed.scope,
            entries: vec![expected.clone()],
            failures: vec![],
        })));
    let recovered = receive(&mut receiver, |view| view.connected).await;
    assert_eq!(recovered.entries, vec![expected]);
    assert!(recovered.error.is_none());
    assert!(feed.subscriptions.load(Ordering::SeqCst) >= 2);
    close(stop, observer).await;
}

#[tokio::test]
async fn rejects_foreign_updates() {
    let expected = entry("fixture", "badge", 0);
    let feed = Feed::new(vec![expected.clone()], vec![]);
    let (mut receiver, stop, observer) = observe(feed.clone());
    receive(&mut receiver, |view| view.connected).await;
    let snapshot = status::Snapshot {
        node: feed.node,
        scope: feed.scope,
        entries: vec![expected.clone()],
        failures: vec![],
    };
    let mut wrong_node = snapshot.clone();
    wrong_node.node = NodeId([2; 32]);
    let mut wrong_worktree = snapshot.clone();
    wrong_worktree.scope.worktree = Some(WorktreeId::new());
    let mut wrong_session = snapshot.clone();
    wrong_session.scope.session = Some(SessionId::new());
    for update in [
        Update::Status(wrong_node),
        Update::Status(wrong_worktree),
        Update::Status(wrong_session),
        Update::ResetRequired,
    ] {
        let _ = feed.updates.send_replace(Ok(update));
        let disconnected = receive(&mut receiver, |view| {
            !view.connected
                && view
                    .error
                    .as_ref()
                    .is_some_and(|fault| fault.code == ErrorCode::WrongTarget)
        })
        .await;
        assert_eq!(disconnected.entries, vec![expected.clone()]);
        let _ = feed
            .updates
            .send_replace(Ok(Update::Status(snapshot.clone())));
        receive(&mut receiver, |view| view.connected).await;
    }
    close(stop, observer).await;
}

#[tokio::test]
async fn empty_scope_never_subscribes() {
    let feed = Feed::new(vec![], vec![]);
    let client = Client::new(feed.clone());
    let (updates, receiver) = watch::channel(View::default());
    client
        .watch_status(
            Scope {
                worktree: None,
                session: None,
            },
            updates,
            CancellationToken::new(),
        )
        .await;
    let view = receiver.borrow();
    assert!(!view.connected);
    assert_eq!(view.error.as_ref().unwrap().code, ErrorCode::InvalidRequest);
    assert_eq!(feed.subscriptions.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn watches_session_without_resolved_worktree() {
    let expected = entry("fixture", "badge", 0);
    let mut feed = Feed::new(vec![], vec![]);
    let owner = Arc::get_mut(&mut feed).unwrap();
    owner.scope.worktree = None;
    let _ = owner
        .updates
        .send_replace(Ok(Update::Status(status::Snapshot {
            node: owner.node,
            scope: owner.scope,
            entries: vec![expected.clone()],
            failures: vec![],
        })));
    let (mut receiver, stop, observer) = observe(feed.clone());
    let view = receive(&mut receiver, |view| view.connected).await;
    assert_eq!(view.entries, vec![expected]);
    assert_eq!(feed.subscriptions.load(Ordering::SeqCst), 1);
    close(stop, observer).await;
}

#[test]
fn prepares_captured_durable_action() {
    let feed = Feed::new(vec![], vec![]);
    let client = Client::new(feed.clone());
    let entry = entry("fixture", "badge", 0);
    let request = client.prepare_status(&feed.scope, &entry).unwrap();
    assert_eq!(request.target, feed.node);
    assert!(request.command.durable());
    assert_eq!(
        request.command,
        Command::CallPlugin {
            handler: "activate".into(),
            input: serde_json::json!({"id": "badge"}),
        }
    );
    assert_eq!(
        request.plugin,
        Some(Context {
            invocation: None,
            turn: None,
            surface: Surface::Composer,
            package: entry.package,
            worktree: feed.scope.worktree,
            session: feed.scope.session,
        })
    );
}

#[test]
fn rejects_unavailable_actions() {
    let feed = Feed::new(vec![], vec![]);
    let client = Client::new(feed.clone());
    let entry = entry("fixture", "badge", 0);
    for changed in [
        Entry {
            invoke: None,
            ..entry.clone()
        },
        {
            let mut changed = entry.clone();
            changed.declaration.kind = Kind::Popover;
            changed
        },
        {
            let mut changed = entry.clone();
            changed.state.visible = false;
            changed
        },
        {
            let mut changed = entry.clone();
            changed.state.enabled = false;
            changed
        },
        {
            let mut changed = entry.clone();
            changed.declaration.slot = Slot::Context;
            changed
        },
        {
            let mut changed = entry.clone();
            changed.state.reply_to = Some(1);
            changed
        },
    ] {
        assert_eq!(
            client
                .prepare_status(&feed.scope, &changed)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    assert_eq!(
        client
            .prepare_status(
                &Scope {
                    worktree: None,
                    session: None
                },
                &entry
            )
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn session_action_requires_resolved_worktree() {
    let feed = Feed::new(vec![], vec![]);
    let client = Client::new(feed.clone());
    let entry = entry("fixture", "badge", 0);
    let scope = Scope {
        worktree: None,
        session: feed.scope.session,
    };
    assert!(scope.valid());
    assert_eq!(
        client.prepare_status(&scope, &entry).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert!(client.prepare_status(&feed.scope, &entry).is_ok());
    assert!(
        client
            .prepare_status(
                &Scope {
                    worktree: feed.scope.worktree,
                    session: None,
                },
                &entry,
            )
            .is_ok()
    );
}
