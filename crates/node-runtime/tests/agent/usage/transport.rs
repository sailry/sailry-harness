use super::*;
use sailry_link::{Admission, Pending, Subscription};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;

pub(super) struct Observed {
    inner: Arc<dyn Transport>,
    mode: AtomicU8,
    reads: AtomicUsize,
    reset: Mutex<CancellationToken>,
    held: Semaphore,
    release: Semaphore,
}

impl Observed {
    pub(super) fn new(inner: Arc<dyn Transport>) -> Self {
        Self {
            inner,
            mode: AtomicU8::new(0),
            reads: AtomicUsize::new(0),
            reset: Mutex::new(CancellationToken::new()),
            held: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }
    pub(super) fn mode(&self, mode: u8) {
        self.mode.store(mode, Ordering::SeqCst);
    }
    pub(super) fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
    pub(super) fn disconnect(&self) {
        self.reset.lock().unwrap().cancel();
    }
    pub(super) async fn held(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.held.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    pub(super) fn release(&self) {
        self.release.add_permits(1);
    }
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let mode = if matches!(request.command, Command::ReadUsage(_)) {
                self.reads.fetch_add(1, Ordering::SeqCst);
                self.mode.swap(0, Ordering::SeqCst)
            } else {
                0
            };
            if mode == 1 {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected usage read failure",
                ));
            }
            let mut admission = self.inner.dispatch(request).await?;
            if mode >= 2 {
                let mut result = admission.completion.await.unwrap();
                if mode == 2 {
                    self.held.add_permits(1);
                    self.release.acquire().await.unwrap().forget();
                } else if let Ok(Output::Usage(report)) = &mut result {
                    if mode == 3 {
                        report.node = NodeId([99; 32]);
                    }
                    if mode == 4 {
                        report.query.start_ms += 1;
                    }
                    if mode == 5 {
                        report.cursor = 0;
                    }
                }
                let (send, receive) = tokio::sync::oneshot::channel();
                send.send(result).unwrap();
                admission.completion = receive;
            }
            Ok(admission)
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            let inner = self.inner.subscribe(topic).await?;
            let reset = CancellationToken::new();
            *self.reset.lock().unwrap() = reset.clone();
            Ok(Box::new(Events { inner, reset }) as Box<dyn Subscription>)
        })
    }
}

struct Events {
    inner: Box<dyn Subscription>,
    reset: CancellationToken,
}
impl Subscription for Events {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            tokio::select! {
                _ = self.reset.cancelled() => Err(Fault::new(ErrorCode::Unavailable, "injected usage subscription disconnect")),
                result = self.inner.next() => result,
            }
        })
    }
}
