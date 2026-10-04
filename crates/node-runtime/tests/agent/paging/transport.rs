use super::*;
use sailry_link::{Admission, Pending, Subscription};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;

pub(crate) struct Observed {
    inner: Arc<dyn Transport>,
    reads: AtomicUsize,
    held: Semaphore,
    release: Semaphore,
    reset: Mutex<CancellationToken>,
    cancelled: Arc<AtomicUsize>,
}

impl Observed {
    pub(crate) fn new(inner: Arc<dyn Transport>) -> Self {
        Self {
            inner,
            reads: AtomicUsize::new(0),
            held: Semaphore::new(0),
            release: Semaphore::new(0),
            reset: Mutex::new(CancellationToken::new()),
            cancelled: Arc::new(AtomicUsize::new(0)),
        }
    }
    pub(crate) fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
    pub(crate) fn cancelled_reads(&self) -> usize {
        self.cancelled.load(Ordering::SeqCst)
    }
    pub(crate) async fn wait_held(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.held.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    pub(crate) fn release(&self) {
        self.release.add_permits(1);
    }
    pub(crate) fn disconnect(&self) {
        self.reset.lock().unwrap().cancel();
    }
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let page = matches!(
                request.command,
                Command::ReadConversation {
                    before: Some(_),
                    ..
                }
            );
            let count = if page {
                self.reads.fetch_add(1, Ordering::SeqCst) + 1
            } else {
                0
            };
            if count == 1 {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected history read failure",
                ));
            }
            let mut admission = self.inner.dispatch(request).await?;
            if count == 2 {
                let result = admission.completion.await.unwrap();
                self.held.add_permits(1);
                self.release.acquire().await.unwrap().forget();
                let (sender, receiver) = tokio::sync::oneshot::channel();
                sender.send(result).unwrap();
                admission.completion = receiver;
            }
            Ok(admission)
        })
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async move {
            let conversation = matches!(topic, Topic::Conversation(_));
            let inner = self.inner.subscribe(topic).await?;
            if !conversation {
                return Ok(inner);
            }
            let reset = CancellationToken::new();
            *self.reset.lock().unwrap() = reset.clone();
            Ok(Box::new(Events {
                inner,
                reset,
                cancelled: self.cancelled.clone(),
            }) as Box<dyn Subscription>)
        })
    }
}

struct Events {
    inner: Box<dyn Subscription>,
    reset: CancellationToken,
    cancelled: Arc<AtomicUsize>,
}

struct Read {
    cancelled: Arc<AtomicUsize>,
    completed: bool,
}

impl Drop for Read {
    fn drop(&mut self) {
        if !self.completed {
            self.cancelled.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Subscription for Events {
    fn next(&mut self) -> Pending<'_, Result<Update, Fault>> {
        Box::pin(async move {
            let mut read = Read {
                cancelled: self.cancelled.clone(),
                completed: false,
            };
            let result = tokio::select! {
                _ = self.reset.cancelled() => Ok(Update::ResetRequired),
                result = self.inner.next() => result,
            };
            read.completed = true;
            result
        })
    }
}
