use std::time::Duration;

use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Snapshot};
use tokio::sync::watch;

use crate::{Apply, Client, Projection};

/// One latest projection for each observer; UI caches are not service history.
#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub snapshot: Option<Snapshot>,
    pub notifications: Vec<crate::activity::Notice>,
    pub connected: bool,
    pub error: Option<Fault>,
}

struct Closing(watch::Sender<View>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| view.connected = false);
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
        let _closing = Closing(updates.clone());
        let mut projection = Projection::new(self.target(), 0);
        let mut activity = crate::activity::Feed::default();
        let mut generation = 0u64;
        let mut delay = Duration::from_millis(250);
        loop {
            let stream = tokio::select! {
                biased;
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                stream = self.subscribe() => stream,
            };
            let outcome = match stream {
                Ok(mut stream) => {
                    generation = generation.checked_add(1).ok_or_else(|| {
                        Fault::new(ErrorCode::Internal, "subscription generation exhausted")
                    })?;
                    projection.reconnect(generation)?;
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
                            let recovered = tokio::select! {
                                biased;
                                _ = stop.cancelled() => return Ok(()),
                                _ = updates.closed() => return Ok(()),
                                recovered = self.recover(&mut projection, generation) => recovered,
                            };
                            if let Err(error) = recovered {
                                break error;
                            }
                        }
                        if applied != Apply::Ignored {
                            delay = Duration::from_millis(250);
                            if let Some(snapshot) = projection.snapshot() {
                                activity.observe(snapshot);
                            }
                            updates.send_replace(View {
                                snapshot: projection.snapshot().cloned(),
                                notifications: activity.notices.clone(),
                                connected: true,
                                error: None,
                            });
                        }
                    }
                }
                Err(error) => error,
            };
            updates.send_replace(View {
                snapshot: projection.snapshot().cloned(),
                notifications: activity.notices.clone(),
                connected: false,
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
    }
}
