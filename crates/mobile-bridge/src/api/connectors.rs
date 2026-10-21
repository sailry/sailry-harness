//! Serializes the shared connection feed without owning connection state or login.
use super::*;
use sailry_link::Subscription;
use sailry_protocol::{Fault, Update, plugin::connector::Changes};
use tokio::{sync::mpsc, task::JoinHandle};

#[cfg(test)]
mod tests;

#[frb(opaque)]
pub struct ConnectionsUpdates {
    receiver: Mutex<mpsc::Receiver<Result<Changes, Fault>>>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl ConnectionsUpdates {
    pub(super) fn new(mut subscription: Box<dyn Subscription>, stop: CancellationToken) -> Self {
        let (sender, receiver) = mpsc::channel(1);
        let cancelled = stop.clone();
        let task = tokio::spawn(async move {
            loop {
                let update = tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => break,
                    result = subscription.next() => result.and_then(|update| match update {
                        Update::ConnectionsChanged(changes) => Ok(changes),
                        _ => Err(Fault::invalid("connection snapshot expected")),
                    }),
                };
                let failed = update.is_err();
                tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => break,
                    result = sender.send(update) => if result.is_err() { break; },
                }
                if failed {
                    break;
                }
            }
        });
        Self {
            receiver: Mutex::new(receiver),
            stop,
            task,
        }
    }

    /// Returns the initial snapshot and subsequent full snapshots from the Node.
    pub async fn next(&self) -> Result<String, String> {
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err("connection subscription is closed".into()),
            result = async {
                let mut receiver = self.receiver.lock().await;
                let changes = receiver.recv().await
                    .ok_or_else(|| "connection subscription is closed".to_owned())?
                    .map_err(error)?;
                serde_json::to_string(&changes).map_err(error)
            } => result,
        }
    }

    pub fn close(&self) {
        self.stop.cancel();
    }
}

impl Drop for ConnectionsUpdates {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
    }
}
