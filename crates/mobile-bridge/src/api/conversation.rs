//! Dart receives the shared conversation projection, not raw engine records.
use super::*;
use sailry_client::{
    Client,
    conversation::{HistoryRequest, View},
};
use sailry_protocol::SessionId;
use std::sync::Arc;
use tokio::{sync::watch, task::JoinHandle};
mod tools;

#[frb(opaque)]
pub struct ConversationUpdates {
    receiver: Mutex<watch::Receiver<View>>,
    current: watch::Receiver<View>,
    details: sailry_client::conversation::details::Tracker,
    detail_changes: Mutex<watch::Receiver<u64>>,
    selected: std::sync::Mutex<Vec<tools::Selection>>,
    sent: std::sync::Mutex<std::collections::BTreeSet<sailry_protocol::tool::detail::Reference>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
    older: tokio::sync::mpsc::Sender<HistoryRequest>,
}

impl ConversationUpdates {
    pub(super) fn new(client: Arc<Client>, session: SessionId, stop: CancellationToken) -> Self {
        let (sender, receiver) = watch::channel(View::default());
        let (older, requests) = tokio::sync::mpsc::channel(8);
        let cancellation = stop.clone();
        let watcher = client.clone();
        let task = tokio::spawn(async move {
            let _ = watcher
                .watch_conversation(session, sender, cancellation, requests)
                .await;
        });
        let details = sailry_client::conversation::details::Tracker::new(
            client.clone(),
            tokio::runtime::Handle::current(),
        );
        let detail_changes = Mutex::new(details.subscribe());
        Self {
            details,
            detail_changes,
            selected: Default::default(),
            sent: Default::default(),
            current: receiver.clone(),
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
                let mut changes = self.detail_changes.lock().await;
                tokio::select! {
                    changed = receiver.changed() => changed.map_err(error)?,
                    changed = changes.changed() => changed.map_err(error)?,
                }
                let current = receiver.borrow_and_update().clone();
                {
                    let selection = self.selected.lock().expect("tool selection lock");
                    tools::select(&current, &selection, &self.details);
                }
                tools::view(&current, Some(&self.details), &mut self.sent.lock().expect("sent details lock"))
            } => result,
        }
    }

    /// Declare mounted tool surfaces; this is controller state, not a Node command.
    pub fn track_tools(&self, selection: String) -> Result<(), String> {
        if self.stop.is_cancelled() {
            return Err("conversation subscription is closed".into());
        }
        let selection: tools::Selected = serde_json::from_str(&selection).map_err(error)?;
        let current = self.current.borrow().clone();
        let mut selected = self.selected.lock().expect("tool selection lock");
        tools::select(&current, &selection.items, &self.details);
        for reference in tools::references(&current, &selection.retry) {
            self.details.retry(&reference);
        }
        *selected = selection.items;
        Ok(())
    }

    pub fn close(&self) {
        self.details.set([]);
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
