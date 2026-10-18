//! Transport telemetry stays controller-local and follows the exact Client transport.
use crate::Client;
use sailry_link::{CancellationToken, ConnectionInfo};
use tokio::sync::watch;

impl Client {
    pub async fn watch_connection(
        &self,
        updates: watch::Sender<ConnectionInfo>,
        stop: CancellationToken,
    ) {
        self.transport.watch_connection(updates, stop).await;
    }
}
