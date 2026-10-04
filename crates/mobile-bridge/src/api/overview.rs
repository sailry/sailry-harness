//! Thin ownership and serialization adapter for the shared multi-Node observer.
use super::*;
use sailry_client::{
    Client,
    usage::{Overview, watch_overview},
};
use sailry_protocol::usage::Query;
use std::sync::Arc;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

impl Controller {
    pub async fn watch_usage(
        &self,
        addresses: Vec<String>,
        query: String,
    ) -> Result<OverviewUpdates, String> {
        self.check()?;
        if addresses.is_empty() {
            return Err("usage overview requires an execution Node".into());
        }
        let query: Query = serde_json::from_str(&query).map_err(error)?;
        let clients = addresses
            .into_iter()
            .map(|address| {
                let address = serde_json::from_str(&address).map_err(error)?;
                Ok(Arc::new(Client::new(self.handle.remote(address))))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let stop = self.stop.child_token();
        let (sender, receiver) = watch::channel(Overview::default());
        let (refresh, requests) = mpsc::channel(1);
        let cancelled = stop.clone();
        let task = tokio::spawn(async move {
            let _ = watch_overview(clients, query, sender, cancelled, requests).await;
        });
        Ok(OverviewUpdates {
            receiver: Mutex::new(receiver),
            refresh,
            stop,
            task,
        })
    }
}

#[frb(opaque)]
pub struct OverviewUpdates {
    receiver: Mutex<watch::Receiver<Overview>>,
    refresh: mpsc::Sender<()>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl OverviewUpdates {
    pub async fn next(&self) -> Result<String, String> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err("usage overview is closed".into()),
            result = async {
                let mut receiver = self.receiver.lock().await;
                receiver.changed().await.map_err(error)?;
                serde_json::to_string(&*receiver.borrow_and_update()).map_err(error)
            } => result,
        }
    }

    pub fn refresh(&self) -> Result<(), String> {
        if self.stop.is_cancelled() {
            return Err("usage overview is closed".into());
        }
        match self.refresh.try_send(()) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(())) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(())) => Err("usage overview is closed".into()),
        }
    }

    pub fn close(&self) {
        self.stop.cancel();
    }
}

impl Drop for OverviewUpdates {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
