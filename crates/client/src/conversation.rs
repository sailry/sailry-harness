mod assets;
mod history;
mod observer;
mod projection;
mod search;
pub mod tools;
pub use projection::Projection;

use crate::Client;
use sailry_link::{CancellationToken, Subscription};
use sailry_protocol::{conversation::Snapshot, *};
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryRequest {
    Older,
    /// Load through an immutable turn admission sequence, such as a search result.
    Through(std::num::NonZeroU64),
}

#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub snapshot: Option<Arc<Snapshot>>,
    pub calls: Arc<Vec<tools::Call>>,
    pub connected: bool,
    pub error: Option<Fault>,
    pub loading_older: bool,
    pub older_error: Option<Fault>,
}

impl View {
    fn replace(&mut self, snapshot: Arc<Snapshot>) {
        if self
            .snapshot
            .as_ref()
            .is_none_or(|previous| !Arc::ptr_eq(&previous.page, &snapshot.page))
        {
            self.calls = Arc::new(tools::collect(&snapshot.page));
        }
        self.snapshot = Some(snapshot);
        self.connected = true;
        self.error = None;
    }
}

struct Closing(watch::Sender<View>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| {
            view.connected = false;
            view.loading_older = false;
        });
    }
}

impl Client {
    pub async fn subscribe_conversation(
        &self,
        session: SessionId,
    ) -> Result<Box<dyn Subscription>, Fault> {
        self.transport.subscribe(Topic::Conversation(session)).await
    }
}
