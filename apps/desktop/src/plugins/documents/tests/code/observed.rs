use sailry_link::{Admission, CancellationToken, Pending, Subscription, Transport};
use sailry_protocol::{Command, ErrorCode, Fault, NodeId, Output, Request, StreamId, Topic};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub(super) struct Observed {
    inner: Arc<dyn Transport>,
    pub requests: Mutex<Vec<Request>>,
    pub lose: AtomicBool,
    pub unknown: AtomicBool,
    pub fail_subscription: AtomicBool,
    pub subscriptions: AtomicUsize,
    original: Mutex<Option<Request>>,
    snapshot: AtomicBool,
    pub entered: CancellationToken,
    pub release: CancellationToken,
}
impl Observed {
    pub fn new(inner: Arc<dyn Transport>) -> Self {
        Self {
            inner,
            requests: Mutex::new(vec![]),
            lose: AtomicBool::new(false),
            unknown: AtomicBool::new(false),
            fail_subscription: AtomicBool::new(false),
            subscriptions: AtomicUsize::new(0),
            original: Mutex::new(None),
            snapshot: AtomicBool::new(false),
            entered: CancellationToken::new(),
            release: CancellationToken::new(),
        }
    }
    pub fn hold_snapshot(&self) -> Release {
        self.snapshot.store(true, Ordering::SeqCst);
        Release(self.release.clone())
    }
    pub fn starts(&self) -> Vec<Request> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| matches!(request.command, Command::StartCodeServer { .. }))
            .cloned()
            .collect()
    }
}
impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn open(&self, id: StreamId) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(id)
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            if matches!(topic, Topic::CodeServers(_)) {
                self.subscriptions.fetch_add(1, Ordering::SeqCst);
                if self.fail_subscription.swap(false, Ordering::SeqCst) {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        "injected code subscription loss",
                    ));
                }
            }
            self.inner.subscribe(topic).await
        })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::Snapshot)
                && self.snapshot.swap(false, Ordering::SeqCst)
            {
                self.entered.cancel();
                self.release.cancelled().await;
            }
            if matches!(
                request.command,
                Command::ReadCode { .. }
                    | Command::CancelCodeQuery { .. }
                    | Command::StartCodeServer { .. }
            ) {
                self.requests.lock().unwrap().push(request.clone());
            }
            let lose = self.lose.load(Ordering::SeqCst);
            let unknown = self.unknown.load(Ordering::SeqCst);
            if unknown && let Command::InspectRequest { id, digest } = &request.command {
                let original = self.original.lock().unwrap().clone().unwrap();
                assert_eq!(*id, original.id);
                assert_eq!(
                    digest,
                    &blake3::hash(&serde_json::to_vec(&original).unwrap())
                        .to_hex()
                        .to_string()
                );
                let (send, completion) = tokio::sync::oneshot::channel();
                send.send(Ok(Output::RequestOutcome {
                    id: *id,
                    outcome: sailry_protocol::RequestOutcome::Completed(Box::new(Err(Fault::new(
                        ErrorCode::OutcomeUnknown,
                        "injected canonical startup uncertainty",
                    )))),
                }))
                .unwrap();
                return Ok(Admission {
                    receipt: sailry_protocol::Receipt {
                        id: request.id,
                        durable: false,
                    },
                    completion,
                });
            }
            if lose && matches!(request.command, Command::InspectRequest { .. }) {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected startup inspection loss",
                ));
            }
            let startup = matches!(request.command, Command::StartCodeServer { .. });
            if unknown && startup {
                *self.original.lock().unwrap() = Some(request.clone());
            }
            let admission = self.inner.dispatch(request).await?;
            if (lose || unknown) && startup {
                admission.completion.await.unwrap().unwrap();
                Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected startup completion loss",
                ))
            } else {
                Ok(admission)
            }
        })
    }
}

pub(super) struct Release(CancellationToken);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
