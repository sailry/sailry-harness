mod assets;
pub mod children;
pub mod details;
pub mod export;
pub mod guidance;
mod history;
#[cfg(test)]
mod identity_tests;
pub mod images;
pub mod links;
pub mod modes;
mod observer;
mod projection;
mod removal;
pub mod reply;
pub mod rows;
pub mod search;
pub mod tools;
pub mod transcript;
pub mod turns;
pub use assets::unique as unique_assets;
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
    /// The existing observer connection generation; snapshots begin a new generation.
    pub generation: u64,
    /// First live canonical entry; older history pages never establish this boundary.
    pub live_start: Option<u64>,
    pub snapshot: Option<Arc<Snapshot>>,
    /// Stable identity for composer metadata while only transient drafts change.
    #[serde(skip)]
    pub statistics: Option<Arc<conversation::Statistics>>,
    pub calls: Arc<Vec<tools::Call>>,
    pub children: Arc<children::View>,
    pub guidance: Arc<std::collections::BTreeMap<TurnId, guidance::State>>,
    /// Shared transcript order for every controller; see `turns::timeline`.
    pub timeline: Arc<Vec<TurnId>>,
    /// True only after authoritative Node observation and confirmation agree.
    pub removed: bool,
    pub connected: bool,
    pub error: Option<Fault>,
    pub statistics_error: Option<Fault>,
    pub loading_older: bool,
    pub older_error: Option<Fault>,
    /// Advances for artifact commits and snapshot recovery, never text deltas.
    pub artifact_cursor: u64,
}

impl View {
    fn replace(
        &mut self,
        snapshot: Arc<Snapshot>,
        artifact_cursor: u64,
        generation: u64,
        live_start: Option<u64>,
    ) {
        if self.statistics.as_deref() != snapshot.statistics.as_ref() {
            self.statistics = snapshot.statistics.clone().map(Arc::new);
        }
        if self
            .snapshot
            .as_ref()
            .is_none_or(|previous| !Arc::ptr_eq(&previous.page, &snapshot.page))
        {
            self.calls = Arc::new(tools::collect(&snapshot.page));
            self.children = Arc::new(children::project(&snapshot.page));
            self.guidance = Arc::new(guidance::project(&snapshot.page));
            self.timeline = Arc::new(turns::timeline(&snapshot.page));
        }
        self.snapshot = Some(snapshot);
        self.generation = generation;
        self.live_start = live_start;
        self.artifact_cursor = artifact_cursor;
        self.connected = true;
        self.error = None;
        self.statistics_error = None;
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
