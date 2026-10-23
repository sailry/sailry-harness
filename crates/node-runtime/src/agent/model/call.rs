//! One model call identity survives generation, recovery and deferred stream polls.
use adk_core::LlmResponseStream;
use futures::Stream;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

tokio::task_local! {
    static CURRENT: Arc<crate::logging::Context>;
}

pub(super) fn current() -> Option<crate::logging::Context> {
    CURRENT.try_with(|context| context.as_ref().clone()).ok()
}

pub(super) fn identity(mut context: crate::logging::Context) -> crate::logging::Context {
    context
        .call
        .get_or_insert_with(sailry_protocol::RequestId::new);
    context
}

pub(super) async fn scope<T>(
    context: crate::logging::Context,
    future: impl Future<Output = T>,
) -> T {
    CURRENT.scope(Arc::new(context), future).await
}

pub(super) fn stream(
    context: crate::logging::Context,
    responses: LlmResponseStream,
) -> LlmResponseStream {
    Box::pin(Responses {
        context: Arc::new(context),
        responses,
    })
}

struct Responses {
    context: Arc<crate::logging::Context>,
    responses: LlmResponseStream,
}

impl Stream for Responses {
    type Item = adk_core::Result<adk_core::LlmResponse>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        CURRENT.sync_scope(this.context.clone(), || {
            this.responses.as_mut().poll_next(cx)
        })
    }
}

#[cfg(test)]
mod tests;
