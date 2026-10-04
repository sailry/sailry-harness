use gpui_shell::{HostError, HostValue};
use sailry_client::{Client, FileView};
use sailry_link::CancellationToken;
use sailry_protocol::WorktreeId;
use std::sync::{Arc, OnceLock};

/// Only coalesced invalidation metadata crosses this adapter. File bytes still
/// use the ordinary plugin-scoped ReadFile command and Node checks.
pub(super) struct Files {
    allowed: bool,
    receiver: OnceLock<tokio::sync::watch::Receiver<FileView>>,
    pending: Arc<tokio::sync::Semaphore>,
}

impl Files {
    pub(super) fn new(allowed: bool) -> Self {
        Self {
            allowed,
            receiver: OnceLock::new(),
            pending: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    pub(super) fn next(
        &self,
        seen: String,
        client: Arc<Client>,
        worktree: WorktreeId,
        runtime: tokio::runtime::Handle,
        stop: CancellationToken,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        if !self.allowed {
            return Err(HostError::new("plugin does not declare file reading"));
        }
        let permit = self
            .pending
            .clone()
            .try_acquire_owned()
            .map_err(|_| HostError::new("a file change request is already pending"))?;
        let mut receiver = self
            .receiver
            .get_or_init(|| {
                let (sender, receiver) = tokio::sync::watch::channel(FileView::default());
                let stop = stop.clone();
                runtime.spawn(async move {
                    let _ = client.watch_files(worktree, sender, stop).await;
                });
                receiver
            })
            .clone();
        Ok(async move {
            let task = runtime.spawn(async move {
                let _permit = permit;
                loop {
                    let value = {
                        let view = receiver.borrow_and_update();
                        serde_json::json!({"revision": view.revision, "connected": view.connected})
                            .to_string()
                    };
                    if value != seen {
                        return Ok(HostValue::from(value));
                    }
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => return Err(HostError::new("plugin view is closed")),
                        result = receiver.changed() => if result.is_err() {
                            return Err(HostError::new("plugin file subscription ended"));
                        },
                    }
                }
            });
            task.await
                .map_err(|_| HostError::new("plugin file subscription worker failed"))?
        })
    }
}
