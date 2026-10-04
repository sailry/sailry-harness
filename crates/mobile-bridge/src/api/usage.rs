//! Thin handles for the shared report observer; aggregation stays on the execution Node.
use super::*;
use sailry_client::{Client, usage::View};
use sailry_protocol::usage::Query;
use std::sync::Arc;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

#[frb(opaque)]
pub struct UsageUpdates {
    receiver: Mutex<watch::Receiver<View>>,
    refresh: mpsc::Sender<()>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl UsageUpdates {
    pub(super) fn new(client: Arc<Client>, query: Query, stop: CancellationToken) -> Self {
        let (sender, receiver) = watch::channel(View::default());
        let (refresh, requests) = mpsc::channel(1);
        let cancelled = stop.clone();
        let task = tokio::spawn(async move {
            let _ = client.watch_usage(query, sender, cancelled, requests).await;
        });
        Self {
            receiver: Mutex::new(receiver),
            refresh,
            stop,
            task,
        }
    }

    pub async fn next(&self) -> Result<String, String> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err("usage subscription is closed".into()),
            result = async {
                let mut receiver = self.receiver.lock().await;
                receiver.changed().await.map_err(error)?;
                serde_json::to_string(&*receiver.borrow_and_update()).map_err(error)
            } => result,
        }
    }

    pub fn refresh(&self) -> Result<(), String> {
        if self.stop.is_cancelled() {
            return Err("usage subscription is closed".into());
        }
        match self.refresh.try_send(()) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(())) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(())) => {
                Err("usage subscription is closed".into())
            }
        }
    }

    pub fn close(&self) {
        self.stop.cancel();
    }
}

impl Drop for UsageUpdates {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
