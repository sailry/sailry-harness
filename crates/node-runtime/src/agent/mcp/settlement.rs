use super::*;
use tokio::sync::{OwnedMutexGuard, oneshot};

pub(super) struct Owner {
    pub connection: Arc<Connected>,
    pub active: input::Active,
    pub service: Option<OwnedMutexGuard<()>>,
    pub serial: OwnedMutexGuard<()>,
    pub shutdown: CancellationToken,
}

impl Owner {
    pub async fn run(
        self,
        name: String,
        arguments: serde_json::Map<String, Value>,
    ) -> Result<Result<CallToolResult, Fault>, ()> {
        let stopped = self.active.stopped();
        let _cancel = stopped.clone().drop_guard();
        let connection = self.connection.clone();
        let (sender, receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            let Self {
                connection,
                active,
                service,
                serial,
                shutdown,
            } = self;
            let _service = service;
            let _serial = serial;
            let operation = connection.toolset.call(&name, arguments, &stopped);
            tokio::pin!(operation);
            let result = tokio::select! {
                biased;
                _ = shutdown.cancelled() => Err(()),
                result = active.run(&mut operation) => result,
            };
            let unfinished = result.is_err();
            let _ = sender.send(result);
            if unfinished {
                stopped.cancel();
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => {
                        let _ = connection.close().await;
                    }
                    _ = &mut operation => {}
                }
            }
            if !connection.closed.load(Ordering::Relaxed) && !connection.healthy() {
                let _ = connection.close().await;
            }
            drop(active);
        });
        connection.retain(task);
        receiver.await.map_err(|_| ())?
    }
}
