//! Shared ownership for MCP transports whose SDK service can outlive its caller.
use rmcp::{
    RoleClient,
    model::ProtocolVersion,
    service::{ClientLifecycleMode, RxJsonRpcMessage, TxJsonRpcMessage},
    transport::Transport,
};
use tokio::sync::oneshot;

pub(crate) fn lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::LATEST],
        legacy_version: Some(ProtocolVersion::LATEST_WITH_INITIALIZE),
    }
}

pub(crate) struct Tracked<T: Transport<RoleClient> + 'static> {
    inner: Option<T>,
    closed: Option<oneshot::Sender<bool>>,
}

impl<T: Transport<RoleClient> + 'static> Tracked<T> {
    pub(crate) fn new(inner: T) -> (Self, oneshot::Receiver<bool>) {
        let (closed, completion) = oneshot::channel();
        (
            Self {
                inner: Some(inner),
                closed: Some(closed),
            },
            completion,
        )
    }
}

impl<T: Transport<RoleClient> + 'static> Drop for Tracked<T> {
    fn drop(&mut self) {
        if let (Some(mut transport), Some(closed)) = (self.inner.take(), self.closed.take()) {
            // The SDK drops its transport on failed or cancelled initialization
            // without awaiting close. Retain that cleanup for the Node owner.
            tokio::spawn(async move {
                let result = transport.close().await.is_ok();
                drop(transport);
                let _ = closed.send(result);
            });
        }
    }
}

impl<T: Transport<RoleClient> + 'static> Transport<RoleClient> for Tracked<T> {
    type Error = T::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleClient>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.inner
            .as_mut()
            .expect("MCP transport is retained")
            .send(item)
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleClient>> {
        self.inner.as_mut()?.receive().await
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        let Some(mut transport) = self.inner.take() else {
            return Ok(());
        };
        let result = transport.close().await;
        drop(transport);
        if let Some(closed) = self.closed.take() {
            let _ = closed.send(result.is_ok());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_latest_with_legacy_fallback() {
        assert!(!ProtocolVersion::LATEST.has_initialize());
        assert!(ProtocolVersion::LATEST_WITH_INITIALIZE.has_initialize());
        assert_eq!(
            lifecycle(),
            ClientLifecycleMode::Auto {
                preferred_versions: vec![ProtocolVersion::LATEST],
                legacy_version: Some(ProtocolVersion::LATEST_WITH_INITIALIZE),
            }
        );
    }
}
