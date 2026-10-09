use super::*;
use sailry_link::{Admission, Pending, Stream, Subscription};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub(super) struct Lost {
    inner: Arc<dyn Transport>,
    pending: AtomicBool,
    paging: AtomicUsize,
}

impl Lost {
    pub(super) fn new(inner: Arc<dyn Transport>) -> Self {
        Self {
            inner,
            pending: AtomicBool::new(true),
            paging: AtomicUsize::new(usize::MAX),
        }
    }

    pub(super) fn fail_paging(&self) {
        self.paging.store(0, Ordering::SeqCst);
    }
}

impl Transport for Lost {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn open(&self, resource: StreamId) -> Pending<'_, std::result::Result<Stream, Fault>> {
        self.inner.open(resource)
    }

    fn subscribe(
        &self,
        topic: Topic,
    ) -> Pending<'_, std::result::Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, std::result::Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(
                request.command,
                Command::ReadConversation {
                    before: Some(_),
                    ..
                }
            ) && self.paging.load(Ordering::SeqCst) != usize::MAX
                && self.paging.fetch_add(1, Ordering::SeqCst) == 1
            {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected history restoration failure",
                ));
            }
            let lose = matches!(
                request.command,
                Command::SubmitTask { .. } | Command::StartSession(_)
            ) && self.pending.swap(false, Ordering::SeqCst);
            let admission = self.inner.dispatch(request).await?;
            if lose {
                assert!(admission.completion.await.unwrap().is_ok());
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "injected loss after durable admission",
                ));
            }
            Ok(admission)
        })
    }
}
