use std::time::Duration;

use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Snapshot};
use tokio::sync::watch;

use crate::{Apply, Client, Projection};

mod shared;
pub use shared::Observers;

/// One latest projection for each observer; UI caches are not service history.
#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub snapshot: Option<Snapshot>,
    pub notifications: Vec<crate::activity::Notice>,
    pub package_cursors: std::collections::BTreeMap<String, u64>,
    pub credential_cursor: u64,
    #[serde(skip)]
    pub settled_cursors: std::collections::BTreeMap<sailry_protocol::WorktreeId, u64>,
    #[serde(skip)]
    pub read_cursors: crate::ReadCursors,
    pub child_targets: std::collections::BTreeMap<String, sailry_protocol::SessionId>,
    pub connected: bool,
    pub connection: sailry_link::ConnectionInfo,
    pub error: Option<Fault>,
}

struct Closing(watch::Sender<View>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| {
            view.connected = false;
            view.connection = Default::default();
        });
    }
}

impl Client {
    /// Own subscription generations and recovery here, not in each presentation.
    /// Reconnecting a read stream never replays a business command.
    pub async fn watch(
        &self,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        if let Some(observers) = &self.observers {
            return observers.watch(self, updates, stop).await;
        }
        self.watch_node(updates, stop).await
    }

    async fn watch_node(
        &self,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let (sender, mut connection) = watch::channel(Default::default());
        let telemetry = self.watch_connection(sender, stop.child_token());
        let merge = async {
            while connection.changed().await.is_ok() {
                let state = *connection.borrow_and_update();
                updates.send_modify(|view| view.connection = state);
            }
        };
        tokio::select! {
            result = self.watch_snapshot(updates.clone(), stop.clone()) => result,
            _ = async { tokio::join!(telemetry, merge); } => Ok(()),
        }
    }

    async fn watch_snapshot(
        &self,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let mut log = crate::observation::Observation::new(self.target(), None);
        let result = async {
            let _closing = Closing(updates.clone());
            let mut projection = Projection::new(self.target(), 0);
            let mut activity = crate::activity::Feed::default();
            let mut generation = 0u64;
            let mut delay = Duration::from_millis(250);
            loop {
                log.connecting(generation.saturating_add(1));
                let stream = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Ok(()),
                    _ = updates.closed() => return Ok(()),
                    stream = self.subscribe() => stream,
                };
                let outcome = match stream {
                    Ok(mut stream) => {
                        log.connected();
                        let mut ready = false;
                        generation = generation.checked_add(1).ok_or_else(|| {
                            Fault::new(ErrorCode::Internal, "subscription generation exhausted")
                        })?;
                        projection.reconnect(generation)?;
                        updates.send_modify(|view| {
                            view.credential_cursor = projection.credential_cursor()
                        });
                        loop {
                            let update = tokio::select! {
                                biased;
                                _ = stop.cancelled() => return Ok(()),
                                _ = updates.closed() => return Ok(()),
                                update = stream.next() => update,
                            };
                            let applied =
                                match update.and_then(|update| projection.apply(generation, update)) {
                                    Ok(applied) => applied,
                                    Err(error) => break error,
                                };
                            if applied == Apply::Recover {
                                log.recovering("snapshot", "sequence_gap");
                                // Drop observations of older credentials before waiting for recovery.
                                updates.send_modify(|view| {
                                    view.credential_cursor = projection.credential_cursor()
                                });
                                let recovered = tokio::select! {
                                    biased;
                                    _ = stop.cancelled() => return Ok(()),
                                    _ = updates.closed() => return Ok(()),
                                    recovered = self.recover_observed(&mut projection, generation, Some(log.call())) => recovered,
                                };
                                if let Err(error) = recovered {
                                    break error;
                                }
                                log.connected();
                            }
                            if applied != Apply::Ignored {
                                delay = Duration::from_millis(250);
                                if let Some(snapshot) = projection.snapshot() {
                                    if !ready {
                                        log.ready(snapshot.cursor);
                                        ready = true;
                                    }
                                    activity.observe(snapshot);
                                }
                                let connection = updates.borrow().connection;
                                updates.send_replace(View {
                                    snapshot: projection.snapshot().cloned(),
                                    notifications: activity.notices.clone(),
                                    package_cursors: projection.package_cursors().clone(),
                                    credential_cursor: projection.credential_cursor(),
                                    settled_cursors: projection.settled_cursors().clone(),
                                    read_cursors: projection.read_cursors(),
                                    child_targets: projection.snapshot().map(crate::conversation::children::targets).unwrap_or_default(),
                                    connected: true,
                                    connection,
                                    error: None,
                                });
                            }
                        }
                    }
                    Err(error) => error,
                };
                log.disconnected(&outcome, delay);
                let connected = outcome.code == ErrorCode::Busy && updates.borrow().connected;
                let connection = if connected {
                    updates.borrow().connection
                } else {
                    Default::default()
                };
                updates.send_replace(View {
                    snapshot: projection.snapshot().cloned(),
                    notifications: activity.notices.clone(),
                    package_cursors: projection.package_cursors().clone(),
                    credential_cursor: projection.credential_cursor(),
                    settled_cursors: projection.settled_cursors().clone(),
                    read_cursors: projection.read_cursors(),
                    child_targets: projection.snapshot().map(crate::conversation::children::targets).unwrap_or_default(),
                    // Backpressure is not evidence that the authenticated transport
                    // or an already-valid projection became unavailable.
                    connected,
                    connection,
                    error: Some(outcome.clone()),
                });
                if !matches!(outcome.code, ErrorCode::Unavailable | ErrorCode::Busy) {
                    return Err(outcome);
                }
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Ok(()),
                    _ = updates.closed() => return Ok(()),
                    _ = tokio::time::sleep(delay) => {},
                }
                delay = (delay * 2).min(Duration::from_secs(5));
            }
        }.await;
        log.finish(result.as_ref().err());
        result
    }
}

#[cfg(test)]
mod tests;
