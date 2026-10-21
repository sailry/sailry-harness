use super::*;
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use std::sync::atomic::{AtomicBool, Ordering};

struct Deferred {
    released: oneshot::Receiver<()>,
    dropped: Arc<AtomicBool>,
}

impl Drop for Deferred {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl Transport<RoleClient> for Deferred {
    type Error = io::Error;
    fn send(
        &mut self,
        _: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = io::Result<()>> + Send + 'static {
        std::future::ready(Ok(()))
    }
    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        None
    }
    async fn close(&mut self) -> io::Result<()> {
        (&mut self.released).await.map_err(|_| failure())
    }
}

type Fixture = (
    Tracked<Deferred>,
    oneshot::Sender<()>,
    oneshot::Receiver<bool>,
    Arc<AtomicBool>,
);

fn fixture() -> Fixture {
    let (release, released) = oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let (transport, completion) = Tracked::new(Deferred {
        released,
        dropped: dropped.clone(),
    });
    (transport, release, completion, dropped)
}

#[tokio::test]
async fn dropping_initialization_joins_cleanup() {
    let (transport, release, mut completion, dropped) = fixture();
    drop(transport);
    tokio::task::yield_now().await;
    assert!(matches!(
        completion.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    assert!(!dropped.load(Ordering::SeqCst));
    release.send(()).unwrap();
    assert!(completion.await.unwrap());
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn reports_cleanup_failure() {
    let (mut transport, release, completion, dropped) = fixture();
    drop(release);
    assert!(transport.close().await.is_err());
    assert!(!completion.await.unwrap());
    assert!(dropped.load(Ordering::SeqCst));
    assert!(transport.close().await.is_ok());
}

#[tokio::test]
async fn retains_guards_through_cleanup() {
    for confirmed in [false, true] {
        for success in [false, true] {
            let (transport, release, completion, dropped) = fixture();
            let stop = CancellationToken::new();
            let closing = CancellationToken::new();
            let connection = Arc::new(tokio::sync::Mutex::new(Connection {
                stop: stop.clone(),
                closing: closing.clone(),
                closed: Some(completion),
                child: None,
                tasks: None,
                result: None,
            }));
            let service = Arc::new(tokio::sync::Mutex::new(()));
            let poisoned = Arc::new(AtomicBool::new(false));
            let mut call = Call::new(
                connection.clone(),
                closing.clone(),
                service.clone().lock_owned().await,
                poisoned.clone(),
            );
            if confirmed {
                call.confirmed();
                assert!(service.try_lock().is_ok());
            }
            drop(call);
            assert_eq!(closing.is_cancelled(), !confirmed);
            assert!(!stop.is_cancelled());
            assert_eq!(poisoned.load(Ordering::Relaxed), !confirmed);
            drop(transport);
            tokio::task::yield_now().await;
            if !confirmed {
                assert!(service.try_lock().is_err());
                assert!(!dropped.load(Ordering::SeqCst));
            }
            if success {
                release.send(()).unwrap();
            } else {
                drop(release);
            }
            let mut owner = connection.lock().await;
            let first = owner.close().await;
            assert!(stop.is_cancelled());
            assert_eq!(first.is_ok(), success);
            assert_eq!(owner.close().await, first);
            assert_eq!(owner.settle().await, first);
            drop(owner);
            let _released = tokio::time::timeout(Duration::from_secs(5), service.lock())
                .await
                .unwrap();
            assert!(dropped.load(Ordering::SeqCst));
        }
    }
}
