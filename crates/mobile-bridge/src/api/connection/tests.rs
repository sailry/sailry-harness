use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{Command, NodeId, Request, Topic};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Backend {
    calls: AtomicUsize,
    entered: tokio::sync::Notify,
}

impl Transport for Backend {
    fn target(&self) -> NodeId {
        NodeId([45; 32])
    }
    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            std::future::pending().await
        })
    }
    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        unreachable!("command execution must not subscribe")
    }
}

#[tokio::test]
async fn rejects_known_failures_without_sending() {
    for closed in [false, true] {
        let backend = Arc::new(Backend::default());
        let connection = Connection::new(
            backend.clone(),
            CancellationToken::new(),
            Default::default(),
        );
        let request = if closed {
            connection
                .prepare(serde_json::to_string(&Command::Snapshot).unwrap())
                .unwrap()
        } else {
            "invalid request".into()
        };
        if closed {
            connection.close();
        }
        let response: sailry_link::Response =
            serde_json::from_str(&connection.execute(request).await.unwrap()).unwrap();
        assert_eq!(
            response.unwrap_err().code,
            if closed {
                ErrorCode::Unavailable
            } else {
                ErrorCode::InvalidRequest
            }
        );
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn cancellation_after_dispatch_remains_uncertain() {
    let backend = Arc::new(Backend::default());
    let connection = Connection::new(
        backend.clone(),
        CancellationToken::new(),
        Default::default(),
    );
    let request = connection
        .prepare(serde_json::to_string(&Command::Snapshot).unwrap())
        .unwrap();
    let (response, ()) = tokio::join!(connection.execute(request), async {
        backend.entered.notified().await;
        connection.close();
    });
    let response: sailry_link::Response = serde_json::from_str(&response.unwrap()).unwrap();
    assert_eq!(response.unwrap_err().code, ErrorCode::OutcomeUnknown);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}
