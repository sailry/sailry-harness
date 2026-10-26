use super::*;
use crate::activity::{Id, Kind, Notice};
use sailry_link::{Admission, Pending, Response, Subscription, Transport};
use sailry_protocol::{Receipt, SessionId, Topic};
use std::{collections::VecDeque, sync::Mutex};

struct Service {
    node: NodeId,
    calls: Mutex<Vec<Request>>,
    replies: Mutex<VecDeque<Option<Response>>>,
}

impl Service {
    fn new(node: NodeId, replies: Vec<Option<Response>>) -> Arc<Self> {
        Arc::new(Self {
            node,
            calls: Mutex::new(Vec::new()),
            replies: Mutex::new(replies.into()),
        })
    }

    fn client(self: &Arc<Self>) -> Arc<Client> {
        Arc::new(Client::new(self.clone()))
    }
}

impl Transport for Service {
    fn target(&self) -> NodeId {
        self.node
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let receipt = Receipt {
                id: request.id,
                durable: true,
            };
            self.calls.lock().unwrap().push(request);
            let (sender, completion) = tokio::sync::oneshot::channel();
            if let Some(reply) = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Some(Ok(Output::NotificationsUpdated)))
            {
                sender.send(reply).unwrap();
            }
            Ok(Admission {
                receipt,
                completion,
            })
        })
    }

    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        unreachable!("notification actions do not subscribe")
    }
}

fn notice(node: NodeId, cursor: u64, target: Target) -> Notice {
    Notice {
        id: Id {
            node,
            cursor,
            target,
        },
        project: None,
        worktree: None,
        title: "Fixture".into(),
        message: String::new(),
        error: None,
        presentation: None,
        kind: Kind::Completed,
        read: false,
        package: String::new(),
        session: None,
        timestamp_ms: None,
        details: None,
    }
}

fn notification(node: NodeId, cursor: u64) -> Notice {
    notice(node, cursor, Target::Notification(NotificationId::new()))
}

#[tokio::test]
async fn groups_updates_by_node() {
    let first = Service::new(NodeId([1; 32]), vec![]);
    let second = Service::new(NodeId([2; 32]), vec![]);
    let clients = BTreeMap::from([(first.node, first.client()), (second.node, second.client())]);
    let mut inbox = Inbox::default();
    for service in [&first, &second] {
        inbox.notices.push(notification(service.node, 1));
        inbox.notices.push(notification(service.node, 2));
    }
    inbox
        .notices
        .push(notice(first.node, 3, Target::Session(SessionId::new())));
    let batch = inbox.prepare(Change::ReadAll);
    let report = batch.run(&clients).await;
    assert_eq!(report.completed, 3);
    assert!(!report.failed());
    inbox.complete(&batch, &report);
    assert_eq!(inbox.unread(), 0);
    for service in [&first, &second] {
        let calls = service.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(matches!(
            &calls[0].command,
            Command::MarkNotificationsRead { ids } if ids.len() == 2
        ));
    }

    let batch = inbox.prepare(Change::DismissRead);
    let report = batch.run(&clients).await;
    inbox.complete(&batch, &report);
    assert!(!report.failed());
    assert_eq!(inbox.notices().len(), 1);
    assert!(matches!(inbox.notices()[0].id.target, Target::Session(_)));
    for service in [&first, &second] {
        let calls = service.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(matches!(
            &calls[1].command,
            Command::DismissReadNotifications { ids } if ids.len() == 2
        ));
    }
}

#[tokio::test]
async fn retains_partial_and_uncertain_requests() {
    let first = Service::new(NodeId([1; 32]), vec![]);
    let second = Service::new(NodeId([2; 32]), vec![None]);
    let third = Service::new(NodeId([3; 32]), vec![]);
    let mut clients =
        BTreeMap::from([(second.node, second.client()), (third.node, third.client())]);
    let mut inbox = Inbox::default();
    for service in [&first, &second, &third] {
        inbox.notices.push(notification(service.node, 1));
    }
    let batch = inbox.prepare(Change::ReadAll);
    let captured = batch.requests.clone();
    let report = batch.run(&clients).await;
    assert_eq!(report.completed, 1);
    assert_eq!(report.errors.len(), 2);
    assert!(report.unknown());
    assert_eq!(report.fault().unwrap().code, ErrorCode::OutcomeUnknown);
    inbox.complete(&batch, &report);
    assert_eq!(inbox.unread(), 2);
    assert_eq!(second.calls.lock().unwrap().len(), 1);
    assert_eq!(third.calls.lock().unwrap().len(), 1);

    let retry = inbox.prepare(Change::ReadAll);
    assert_eq!(retry.requests, captured[..2]);
    clients.insert(first.node, first.client());
    let report = retry.run(&clients).await;
    inbox.complete(&retry, &report);
    assert_eq!(report.completed, 2);
    assert!(!report.failed());
    assert_eq!(inbox.unread(), 0);
    let calls = second.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0], calls[1]);
    assert_eq!(third.calls.lock().unwrap().len(), 1);
    assert!(inbox.prepare(Change::ReadAll).is_empty());
}

#[tokio::test]
async fn preserves_captures_when_notices_arrive() {
    let service = Service::new(NodeId([1; 32]), vec![None]);
    let clients = BTreeMap::from([(service.node, service.client())]);
    let mut inbox = Inbox::default();
    inbox.notices.push(notification(service.node, 1));
    let original = inbox.prepare(Change::ReadAll);
    let report = original.run(&clients).await;
    inbox.complete(&original, &report);
    inbox.notices.push(notification(service.node, 2));

    let retry = inbox.prepare(Change::ReadAll);
    assert_eq!(retry.requests.len(), 2);
    assert_eq!(retry.requests[0], original.requests[0]);
    assert_ne!(retry.requests[1].id, original.requests[0].id);
    assert_eq!(ids(&retry.requests[1]).len(), 1);
    assert_eq!(retry.nodes().count(), 1);
    let report = retry.run(&clients).await;
    inbox.complete(&retry, &report);
    assert!(!report.failed());
    assert_eq!(inbox.unread(), 0);
}

#[tokio::test]
async fn renews_settled_failures_on_explicit_retry() {
    let service = Service::new(
        NodeId([1; 32]),
        vec![Some(Err(Fault::new(
            ErrorCode::Busy,
            "notification update failed",
        )))],
    );
    let clients = BTreeMap::from([(service.node, service.client())]);
    let mut inbox = Inbox::default();
    inbox.notices.push(notification(service.node, 1));
    let original = inbox.prepare(Change::ReadAll);
    let report = original.run(&clients).await;
    assert!(report.failed());
    assert!(!report.unknown());
    inbox.complete(&original, &report);
    assert_eq!(inbox.unread(), 1);

    let retry = inbox.prepare(Change::ReadAll);
    assert_ne!(retry.requests[0].id, original.requests[0].id);
    assert_eq!(retry.requests[0].command, original.requests[0].command);
    let report = retry.run(&clients).await;
    inbox.complete(&retry, &report);
    assert!(!report.failed());
    assert_eq!(inbox.unread(), 0);
}
