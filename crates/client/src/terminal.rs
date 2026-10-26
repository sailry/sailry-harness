mod projection;
pub use projection::Projection;

use crate::{Apply, Client};
use sailry_link::{CancellationToken, Subscription};
use sailry_protocol::{terminal::Snapshot, *};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

#[derive(Clone, Default, serde::Serialize)]
pub struct View {
    pub snapshot: Option<Arc<Snapshot>>,
    pub connected: bool,
    pub recovering: bool,
    pub error: Option<Fault>,
}

struct Closing(watch::Sender<View>);
impl Drop for Closing {
    fn drop(&mut self) {
        self.0.send_modify(|view| {
            view.connected = false;
            view.recovering = false;
        });
    }
}

impl Client {
    pub async fn subscribe_terminal(&self, id: TerminalId) -> Result<Box<dyn Subscription>, Fault> {
        self.transport.subscribe(Topic::Terminal(id)).await
    }

    /// Screen recovery never claims input ownership or replays input commands.
    pub async fn watch_terminal(
        &self,
        id: TerminalId,
        updates: watch::Sender<View>,
        stop: CancellationToken,
    ) -> Result<(), Fault> {
        let _closing = Closing(updates.clone());
        let mut projection = Projection::new(self.target(), id, 0);
        let mut generation = 0;
        let mut delay = Duration::from_millis(250);
        loop {
            generation += 1;
            projection.reconnect(generation)?;
            let stream = tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                stream = self.subscribe_terminal(id) => stream,
            };
            let failure = match stream {
                Ok(mut stream) => loop {
                    let update = tokio::select! {
                        _ = stop.cancelled() => return Ok(()),
                        _ = updates.closed() => return Ok(()),
                        update = stream.next() => update,
                    };
                    match update.and_then(|update| projection.apply(generation, update)) {
                        Ok(Apply::Applied) => {
                            updates.send_replace(View {
                                snapshot: projection.snapshot().cloned().map(Arc::new),
                                connected: true,
                                recovering: false,
                                error: None,
                            });
                            delay = Duration::from_millis(250);
                        }
                        Ok(Apply::Ignored) => {}
                        Ok(Apply::Recover) => break None,
                        Err(error) => break Some(error),
                    }
                },
                Err(error) => Some(error),
            };
            updates.send_modify(|view| {
                view.connected = false;
                view.recovering = failure.is_none();
                view.error = failure.clone();
            });
            if let Some(failure) = failure
                && !matches!(failure.code, ErrorCode::Unavailable | ErrorCode::Busy)
            {
                return Err(failure);
            }
            tokio::select! {
                _ = stop.cancelled() => return Ok(()),
                _ = updates.closed() => return Ok(()),
                _ = tokio::time::sleep(delay) => {},
            }
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    }
}
