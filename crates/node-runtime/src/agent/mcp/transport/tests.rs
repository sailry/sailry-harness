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
