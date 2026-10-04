//! Dart receives the shared conversation projection, not raw ADK events.
use super::*;
use sailry_client::{
    Client,
    conversation::{HistoryRequest, View},
};
use sailry_protocol::SessionId;
use std::sync::Arc;
use tokio::{sync::watch, task::JoinHandle};

#[frb(opaque)]
pub struct ConversationUpdates {
    receiver: Mutex<watch::Receiver<View>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
    older: tokio::sync::mpsc::Sender<HistoryRequest>,
}

impl ConversationUpdates {
    pub(super) fn new(client: Arc<Client>, session: SessionId, stop: CancellationToken) -> Self {
        let (sender, receiver) = watch::channel(View::default());
        let (older, requests) = tokio::sync::mpsc::channel(8);
        let cancellation = stop.clone();
        let task = tokio::spawn(async move {
            let _ = client
                .watch_conversation(session, sender, cancellation, requests)
                .await;
        });
        Self {
            receiver: Mutex::new(receiver),
            stop,
            task,
            older,
        }
    }

    pub async fn next(&self) -> Result<String, String> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err("conversation subscription is closed".into()),
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

    /// Request the next complete-turn page; observe progress and errors through `next`.
    pub fn load_older(&self) -> Result<(), String> {
        use tokio::sync::mpsc::error::TrySendError;
        if self.stop.is_cancelled() {
            return Err("conversation subscription is closed".into());
        }
        match self.older.try_send(HistoryRequest::Older) {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Closed(_)) => Err("conversation subscription is closed".into()),
        }
    }

    /// Load complete turns through a search result's turn sequence, using the same observer.
    pub fn load_through(&self, sequence: u64) -> Result<(), String> {
        use tokio::sync::mpsc::error::TrySendError;
        if self.stop.is_cancelled() {
            return Err("conversation subscription is closed".into());
        }
        let sequence = std::num::NonZeroU64::new(sequence)
            .ok_or("conversation turn sequence must be positive")?;
        match self.older.try_send(HistoryRequest::Through(sequence)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err("conversation history request queue is full".into()),
            Err(TrySendError::Closed(_)) => Err("conversation subscription is closed".into()),
        }
    }
}

impl Drop for ConversationUpdates {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
