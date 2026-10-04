//! Adapts the shared login projection without owning authorization or credentials.
use super::*;
use sailry_client::{Client, login::View};
use sailry_protocol::conversation::login::Attempt;
use std::sync::Arc;
use tokio::{sync::watch, task::JoinHandle};

#[frb(opaque)]
pub struct LoginUpdates {
    receiver: Mutex<Receiver>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

enum Receiver {
    Provider(watch::Receiver<View>),
    Mcp(watch::Receiver<sailry_client::mcp_login::View>),
}

impl LoginUpdates {
    pub(super) fn new(client: Arc<Client>, attempt: Attempt, stop: CancellationToken) -> Self {
        let (sender, receiver) = watch::channel(View::default());
        let cancelled = stop.clone();
        let task = tokio::spawn(async move {
            let _ = client.watch_login(attempt, sender, cancelled).await;
        });
        Self {
            receiver: Mutex::new(Receiver::Provider(receiver)),
            stop,
            task,
        }
    }

    pub(super) fn mcp(
        client: Arc<Client>,
        attempt: sailry_protocol::plugin::authorization::Attempt,
        stop: CancellationToken,
    ) -> Self {
        let (sender, receiver) = watch::channel(sailry_client::mcp_login::View::default());
        let cancelled = stop.clone();
        let task = tokio::spawn(async move {
            let _ = client.watch_mcp_login(attempt, sender, cancelled).await;
        });
        Self {
            receiver: Mutex::new(Receiver::Mcp(receiver)),
            stop,
            task,
        }
    }

    pub async fn next(&self) -> Result<String, String> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err("login subscription is closed".into()),
            result = async {
                let mut receiver = self.receiver.lock().await;
                match &mut *receiver {
                    Receiver::Provider(receiver) => {
                        receiver.changed().await.map_err(error)?;
                        serde_json::to_string(&*receiver.borrow_and_update()).map_err(error)
                    }
                    Receiver::Mcp(receiver) => {
                        receiver.changed().await.map_err(error)?;
                        serde_json::to_string(&*receiver.borrow_and_update()).map_err(error)
                    }
                }
            } => result,
        }
    }

    /// Releases observation; cancel the attempt explicitly to stop Node authorization.
    pub fn close(&self) {
        self.stop.cancel();
    }
}

impl Drop for LoginUpdates {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
