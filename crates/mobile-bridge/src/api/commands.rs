//! Command discovery adapts the shared observer without another recovery loop.
use super::*;
use sailry_client::{Client, commands::View};
use sailry_protocol::SessionId;
use std::sync::Arc;
use tokio::{sync::watch, task::JoinHandle};

#[frb(opaque)]
pub struct CommandUpdates {
    receiver: Mutex<watch::Receiver<View>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}
impl CommandUpdates {
    pub(super) fn new(client: Arc<Client>, session: SessionId, stop: CancellationToken) -> Self {
        let (updates, receiver) = watch::channel(View::default());
        let cancellation = stop.clone();
        let task = tokio::spawn(async move {
            let (_selection, selected) = watch::channel(None);
            client
                .watch_commands(session, selected, updates, cancellation)
                .await;
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
            _ = self.stop.cancelled() => Err("command subscription is closed".into()),
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
impl Drop for CommandUpdates {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
