use sailry_link::{Admission, Pending, Stream, Subscription, Transport};
use sailry_protocol::{Command, NodeId};
use sailry_protocol::{Fault, Request, StreamId, Topic};
use std::sync::{Arc, atomic::Ordering};
use std::sync::{Mutex, atomic::AtomicUsize};

pub(super) struct Observed {
    pub inner: Arc<dyn Transport>,
    pub requests: Mutex<Vec<Request>>,
    pub mode: AtomicUsize,
    pub opened: AtomicUsize,
    pub completed: AtomicUsize,
    pub release: Arc<tokio::sync::Semaphore>,
}

impl Observed {
    pub fn new(inner: Arc<dyn Transport>) -> Self {
        Self {
            inner,
            requests: Mutex::new(Vec::new()),
            mode: AtomicUsize::new(0),
            opened: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
            release: Arc::new(tokio::sync::Semaphore::new(0)),
        }
    }

    pub fn release_on_drop(self: &Arc<Self>) -> Release {
        Release(self.clone())
    }
}

pub(super) struct Release(Arc<Observed>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.mode.store(0, Ordering::SeqCst);
        self.0.release.add_permits(64);
    }
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn open(&self, stream: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        Box::pin(async move {
            self.opened.fetch_add(1, Ordering::SeqCst);
            if self.mode.load(Ordering::SeqCst) == 1 {
                self.release.acquire().await.unwrap().forget();
            }
            self.inner.open(stream).await
        })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let finishing = matches!(request.command, Command::FinishFileUpload { .. });
            if finishing || matches!(request.command, Command::UploadFile(_)) {
                self.requests.lock().unwrap().push(request.clone());
            }
            let mut admission = self.inner.dispatch(request).await?;
            let mode = self.mode.load(Ordering::SeqCst);
            if finishing && mode >= 2 {
                let release = self.release.clone();
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                let result = completion.await;
                self.completed.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    if mode == 2 {
                        release.acquire().await.unwrap().forget();
                    }
                    if mode != 3
                        && let Ok(result) = result
                    {
                        let _ = sender.send(result);
                    }
                });
            }
            Ok(admission)
        })
    }
}
