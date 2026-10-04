use super::*;
use sailry_link::{Admission, Pending, Subscription as Stream, Transport};
use sailry_protocol::{Fault, Request, Topic};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

// Only delay or drop delivery; every command still executes on the real Node.
pub(crate) struct Observed {
    pub inner: Arc<dyn Transport>,
    pub requests: Mutex<Vec<Request>>,
    pub hold: AtomicBool,
    pub drop_next: AtomicBool,
    pub ready: Arc<tokio::sync::Semaphore>,
}

impl Transport for Observed {
    fn open(
        &self,
        stream: sailry_protocol::StreamId,
    ) -> Pending<'_, Result<sailry_link::Stream, Fault>> {
        self.inner.open(stream)
    }
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Stream>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let tracked = matches!(
                request.command,
                Command::RenameEntry { .. }
                    | Command::TrashEntry { .. }
                    | Command::CopyEntry { .. }
                    | Command::CopyEntryTo { .. }
                    | Command::MoveEntryTo { .. }
                    | Command::ListFileCheckpoints { .. }
                    | Command::ReadFileCheckpoint { .. }
                    | Command::RestoreFileCheckpoint { .. }
            );
            let hold = tracked && self.hold.load(Ordering::SeqCst);
            let drop_result = tracked && self.drop_next.swap(false, Ordering::SeqCst);
            if tracked {
                self.requests.lock().unwrap().push(request.clone());
            }
            let mut admission = self.inner.dispatch(request).await?;
            if hold || drop_result {
                let ready = self.ready.clone();
                let (sender, receiver) = tokio::sync::oneshot::channel();
                let completion = std::mem::replace(&mut admission.completion, receiver);
                tokio::spawn(async move {
                    let result = completion.await;
                    if hold {
                        ready.acquire().await.unwrap().forget();
                    }
                    if !drop_result && let Ok(result) = result {
                        let _ = sender.send(result);
                    }
                });
            }
            Ok(admission)
        })
    }
}
