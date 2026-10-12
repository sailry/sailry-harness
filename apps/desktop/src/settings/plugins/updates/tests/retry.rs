use super::super::{query, retry_delay};
use sailry_client::Client;
use sailry_link::{Admission, CancellationToken, Pending, Subscription, Transport};
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Receipt, Request, Topic};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Fixture {
    failures: Mutex<VecDeque<ErrorCode>>,
    requests: Mutex<Vec<Request>>,
}

impl Transport for Fixture {
    fn target(&self) -> NodeId {
        NodeId([4; 32])
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            if let Some(code) = self.failures.lock().unwrap().pop_front() {
                return Err(Fault::new(code, "injected inspection failure"));
            }
            let (reply, completion) = tokio::sync::oneshot::channel();
            reply
                .send(Ok(Output::PluginUpdates(Default::default())))
                .unwrap();
            Ok(Admission {
                receipt: Receipt {
                    id: request.id,
                    durable: false,
                },
                completion,
            })
        })
    }

    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async {
            Err(Fault::new(
                ErrorCode::Internal,
                "unused fixture subscription",
            ))
        })
    }
}

fn fixture(failures: impl IntoIterator<Item = ErrorCode>) -> (Arc<Fixture>, Client, Request) {
    let transport = Arc::new(Fixture {
        failures: Mutex::new(failures.into_iter().collect()),
        requests: Mutex::new(Vec::new()),
    });
    let client = Client::new(transport.clone());
    let request = client.prepare(Command::CheckPluginUpdates {
        packages: vec![sailry_protocol::plugin::updates::Request {
            name: "example".into(),
            expected_revision: 7,
        }],
    });
    (transport, client, request)
}

#[core::prelude::v1::test]
fn delays_are_bounded_and_startup_only() {
    for (retries, millis) in [250, 500, 1000, 2000].into_iter().enumerate() {
        assert_eq!(
            retry_delay(true, ErrorCode::Busy, retries as u8),
            Some(Duration::from_millis(millis))
        );
        assert_eq!(retry_delay(false, ErrorCode::Busy, retries as u8), None);
    }
    assert_eq!(retry_delay(true, ErrorCode::Busy, 4), None);
    for code in [
        ErrorCode::Unavailable,
        ErrorCode::OutcomeUnknown,
        ErrorCode::RevisionConflict,
        ErrorCode::InvalidRequest,
    ] {
        assert_eq!(retry_delay(true, code, 0), None);
    }
}

#[tokio::test]
async fn retries_with_captured_request() {
    let (transport, client, request) = fixture([ErrorCode::Busy]);
    let result = query(&client, request.clone(), true, &CancellationToken::new())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(result, Output::PluginUpdates(batch) if batch.reports.is_empty() && batch.failures.is_empty())
    );
    assert_eq!(
        *transport.requests.lock().unwrap(),
        vec![request.clone(), request]
    );
}

#[tokio::test]
async fn stops_after_four_retries() {
    let (transport, client, request) = fixture([ErrorCode::Busy; 6]);
    assert_eq!(
        query(&client, request.clone(), true, &CancellationToken::new())
            .await
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(*transport.requests.lock().unwrap(), vec![request; 5]);
}

#[tokio::test]
async fn does_not_retry_other_failures() {
    for code in [
        ErrorCode::Unavailable,
        ErrorCode::OutcomeUnknown,
        ErrorCode::RevisionConflict,
        ErrorCode::InvalidRequest,
    ] {
        let (transport, client, request) = fixture([code]);
        assert_eq!(
            query(&client, request.clone(), true, &CancellationToken::new())
                .await
                .unwrap()
                .unwrap_err()
                .code,
            code
        );
        assert_eq!(*transport.requests.lock().unwrap(), vec![request]);
    }
    let (transport, client, request) = fixture([ErrorCode::Busy]);
    assert_eq!(
        query(&client, request.clone(), false, &CancellationToken::new())
            .await
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(*transport.requests.lock().unwrap(), vec![request]);
}

#[tokio::test]
async fn cancellation_before_dispatch() {
    let (transport, client, request) = fixture([ErrorCode::Busy]);
    let stop = CancellationToken::new();
    stop.cancel();
    assert!(query(&client, request, true, &stop).await.is_none());
    assert!(transport.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancellation_during_delay() {
    let (transport, client, request) = fixture([ErrorCode::Busy]);
    let stop = CancellationToken::new();
    let mut check = Box::pin(query(&client, request.clone(), true, &stop));
    assert!(futures::poll!(&mut check).is_pending());
    assert_eq!(*transport.requests.lock().unwrap(), vec![request]);
    stop.cancel();
    assert!(check.await.is_none());
    assert_eq!(transport.requests.lock().unwrap().len(), 1);
}
