use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use sailry_link::{Admission, Pending, Response, Subscription, Transport};
use sailry_protocol::{NodeId, Receipt, RequestId, Topic, WorktreeId};

use super::*;

struct Stub {
    original: Request,
    response: Option<Response>,
    calls: AtomicUsize,
}

impl Transport for Stub {
    fn target(&self) -> NodeId {
        self.original.target
    }

    fn dispatch(&self, query: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_ne!(query.id, self.original.id);
            assert_eq!(query.target, self.original.target);
            assert_eq!(
                query.command,
                Command::InspectRequest {
                    id: self.original.id,
                    digest: blake3::hash(&serde_json::to_vec(&self.original).unwrap())
                        .to_hex()
                        .to_string(),
                }
            );
            assert!(
                !serde_json::to_string(&query)
                    .unwrap()
                    .contains("private-text-marker")
            );
            let (sender, completion) = tokio::sync::oneshot::channel();
            if let Some(response) = &self.response {
                sender.send(response.clone()).unwrap();
            }
            Ok(Admission {
                receipt: Receipt {
                    id: query.id,
                    durable: false,
                },
                completion,
            })
        })
    }

    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        unreachable!("outcome queries do not subscribe")
    }
}

#[tokio::test]
async fn preserves_uncertain_outcomes() {
    let original = Request::new(
        NodeId([1; 32]),
        Command::WriteFile {
            worktree: WorktreeId::new(),
            path: "file".into(),
            text: "private-text-marker".into(),
            expected_revision: None,
        },
    );
    for (response, expected) in [
        (
            Some(Ok(Output::RequestOutcome {
                id: original.id,
                outcome: RequestOutcome::NotAdmitted,
            })),
            Ok(RequestOutcome::NotAdmitted),
        ),
        (
            Some(Ok(Output::RequestOutcome {
                id: RequestId::new(),
                outcome: RequestOutcome::NotAdmitted,
            })),
            Err(ErrorCode::Internal),
        ),
        (
            Some(Ok(Output::EntryTrashed {
                path: "file".into(),
            })),
            Err(ErrorCode::Internal),
        ),
        (None, Err(ErrorCode::OutcomeUnknown)),
        (
            Some(Err(Fault::new(ErrorCode::Unavailable, "connection lost"))),
            Err(ErrorCode::Unavailable),
        ),
    ] {
        let client = Client::new(Arc::new(Stub {
            original: original.clone(),
            response,
            calls: AtomicUsize::new(0),
        }));
        assert_eq!(
            client.outcome(&original).await.map_err(|error| error.code),
            expected
        );
    }
}

#[tokio::test]
async fn unsupported_completions_are_not_replayed() {
    let original = Request::new(
        NodeId([1; 32]),
        Command::WriteFile {
            worktree: WorktreeId::new(),
            path: "file".into(),
            text: "private-text-marker".into(),
            expected_revision: None,
        },
    );
    for output in [
        Output::Unsupported,
        Output::PluginTransaction(vec![Output::Unsupported]),
        Output::RequestOutcome {
            id: original.id,
            outcome: RequestOutcome::Completed(Box::new(Ok(Output::Unsupported))),
        },
    ] {
        let transport = Arc::new(Stub {
            original: original.clone(),
            response: Some(Ok(output)),
            calls: AtomicUsize::new(0),
        });
        let client = Client::new(transport.clone());
        assert_eq!(
            client.outcome(&original).await.unwrap_err(),
            Fault::new(
                ErrorCode::Unavailable,
                "request result is not supported; the request will not be replayed",
            )
        );
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    }

    let fault = Fault::new(ErrorCode::Unavailable, "original completion fault");
    let transport = Arc::new(Stub {
        original: original.clone(),
        response: Some(Err(fault.clone())),
        calls: AtomicUsize::new(0),
    });
    assert_eq!(
        Client::new(transport.clone()).outcome(&original).await,
        Err(fault)
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}
