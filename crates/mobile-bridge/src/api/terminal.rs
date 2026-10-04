//! Mobile subscriptions forward the shared Client projection without Dart reduction.
use super::*;
use sailry_client::{Client, terminal::View};
use sailry_protocol::TerminalId;
use std::sync::Arc;
use tokio::{sync::watch, task::JoinHandle};

#[frb(opaque)]
pub struct TerminalUpdates {
    receiver: Mutex<watch::Receiver<View>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl TerminalUpdates {
    pub(super) fn new(client: Arc<Client>, terminal: TerminalId, stop: CancellationToken) -> Self {
        let (sender, receiver) = watch::channel(View::default());
        let cancellation = stop.clone();
        let task = tokio::spawn(async move {
            let _ = client.watch_terminal(terminal, sender, cancellation).await;
        });
        Self {
            receiver: Mutex::new(receiver),
            stop,
            task,
        }
    }

    pub async fn next(&self) -> Result<String, String> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err("terminal subscription is closed".into()),
            result = async {
                let mut receiver = self.receiver.lock().await;
                receiver.changed().await.map_err(error)?;
                serde_json::to_string(&*receiver.borrow_and_update()).map_err(error)
            } => result,
        }
    }

    pub fn close(&self) {
        self.stop.cancel();
    }
}

impl Drop for TerminalUpdates {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
