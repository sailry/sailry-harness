use super::*;
use crate::Client;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{Receipt, Topic, WorktreeId};
use std::{
    collections::VecDeque,
    future::{Future, poll_fn},
    task::Poll,
};

struct Backend {
    node: NodeId,
    calls: Mutex<Vec<Command>>,
    errors: Mutex<VecDeque<ErrorCode>>,
    entered: Semaphore,
    release: Semaphore,
}

impl Backend {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            node: NodeId([next_byte(); 32]),
            calls: Default::default(),
            errors: Default::default(),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        })
    }
    fn client(self: &Arc<Self>, reads: &Arc<Reads>) -> Client {
        Client::new(self.clone()).with_git_reads(reads.clone())
    }
    async fn entered(&self) {
        self.entered.acquire().await.unwrap().forget();
    }
}

fn next_byte() -> u8 {
    use std::sync::atomic::{AtomicU8, Ordering};
    static NEXT: AtomicU8 = AtomicU8::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Transport for Backend {
    fn target(&self) -> NodeId {
        self.node
    }
    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async { Err(Fault::new(ErrorCode::Unavailable, "unused subscription")) })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let index = {
                let mut calls = self.calls.lock().unwrap();
                calls.push(request.command.clone());
                calls.len()
            };
            if is_read(&request.command) {
                self.entered.add_permits(1);
                self.release.acquire().await.unwrap().forget();
            }
            if let Some(error) = self.errors.lock().unwrap().pop_front() {
                return Err(Fault::new(error, "injected read failure"));
            }
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender
                .send(Ok(Output::GitOutput {
                    text: index.to_string(),
                }))
                .unwrap();
            Ok(Admission {
                receipt: Receipt {
                    id: request.id,
                    durable: request.command.durable(),
                },
                completion,
            })
        })
    }
}

#[tokio::test]
async fn coalesces_pending_reads() {
    let backend = Backend::new();
    let reads = Arc::new(Reads::default());
    let conversation = backend.client(&reads);
    let sidebar = backend.client(&reads);
    let command = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    let (left, right, _) = tokio::join!(
        conversation.execute(conversation.prepare(command.clone())),
        sidebar.execute(sidebar.prepare(command.clone())),
        async {
            backend.entered().await;
            backend.release.add_permits(1);
        }
    );
    assert_eq!(left.unwrap(), right.unwrap());
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
    let (next, _) = tokio::join!(sidebar.execute(sidebar.prepare(command)), async {
        backend.entered().await;
        backend.release.add_permits(1);
    });
    assert_eq!(next.unwrap(), Output::GitOutput { text: "2".into() });
}

#[tokio::test]
async fn queues_distinct_worktree_reads() {
    let backend = Backend::new();
    let client = backend.client(&Arc::default());
    let first = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    let second = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    let (left, right, _) = tokio::join!(
        client.execute(client.prepare(first)),
        client.execute(client.prepare(second)),
        async {
            backend.entered().await;
            tokio::task::yield_now().await;
            assert_eq!(
                backend.calls.lock().unwrap().len(),
                1,
                "distinct reads queue at the shared owner"
            );
            backend.release.add_permits(1);
            backend.entered().await;
            backend.release.add_permits(1);
        }
    );
    assert_ne!(left.unwrap(), right.unwrap());
    assert_eq!(backend.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn nodes_read_independently() {
    let first = Backend::new();
    let second = Backend::new();
    let reads = Arc::default();
    let local = first.client(&reads);
    let remote = second.client(&reads);
    let command = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    let (left, right, _) = tokio::join!(
        local.execute(local.prepare(command.clone())),
        remote.execute(remote.prepare(command)),
        async {
            tokio::time::timeout(Duration::from_secs(1), async {
                first.entered().await;
                second.entered().await;
            })
            .await
            .unwrap();
            first.release.add_permits(1);
            second.release.add_permits(1);
        }
    );
    left.unwrap();
    right.unwrap();
}

#[tokio::test]
async fn retries_only_transient_reads() {
    let backend = Backend::new();
    let client = backend.client(&Arc::default());
    backend
        .errors
        .lock()
        .unwrap()
        .extend([ErrorCode::Busy, ErrorCode::Unavailable]);
    let command = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    backend.release.add_permits(3);
    client
        .execute(client.prepare(command.clone()))
        .await
        .unwrap();
    assert_eq!(backend.calls.lock().unwrap().len(), 3);
    backend
        .errors
        .lock()
        .unwrap()
        .push_back(ErrorCode::NotFound);
    backend.release.add_permits(1);
    assert_eq!(
        client
            .execute(client.prepare(command))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(backend.calls.lock().unwrap().len(), 4);
    backend.errors.lock().unwrap().push_back(ErrorCode::Busy);
    let write = Command::RegisterProject {
        name: "Fixture".into(),
        path: "/fixture".into(),
    };
    assert_eq!(
        client
            .execute(client.prepare(write))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy
    );
    assert_eq!(backend.calls.lock().unwrap().len(), 5);
}

#[tokio::test]
async fn shares_read_lifetime() {
    let backend = Backend::new();
    let reads = Arc::default();
    let first = backend.client(&reads);
    let second = backend.client(&reads);
    let command = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    let request = first.prepare(command.clone());
    let task = tokio::spawn(async move { first.execute(request).await });
    backend.entered().await;
    let mut pending = Box::pin(second.execute(second.prepare(command)));
    poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    backend.release.add_permits(1);
    pending.await.unwrap();
    assert_eq!(backend.calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancellation_releases_queue() {
    let backend = Backend::new();
    let reads = Arc::default();
    let client = backend.client(&reads);
    let command = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    let request = client.prepare(command.clone());
    let task = tokio::spawn(async move { client.execute(request).await });
    backend.entered().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let client = backend.client(&reads);
    tokio::time::timeout(Duration::from_secs(1), async {
        let (result, _) = tokio::join!(client.execute(client.prepare(command)), async {
            backend.entered().await;
            backend.release.add_permits(1);
        });
        result.unwrap();
    })
    .await
    .unwrap();
    assert_eq!(backend.calls.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn mutations_order_refreshes() {
    let backend = Backend::new();
    let reads = Arc::default();
    let first = backend.client(&reads);
    let client = backend.client(&reads);
    let command = Command::InspectGit {
        worktree: WorktreeId::new(),
    };
    let request = first.prepare(command.clone());
    let task = tokio::spawn(async move { first.execute(request).await });
    backend.entered().await;
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Fixture".into(),
            path: "/fixture".into(),
        }))
        .await
        .unwrap();
    let (new, _) = tokio::join!(client.execute(client.prepare(command)), async {
        backend.release.add_permits(1);
        backend.entered().await;
        backend.release.add_permits(1);
    });
    assert_ne!(task.await.unwrap().unwrap(), new.unwrap());
    assert_eq!(backend.calls.lock().unwrap().len(), 3);
}
