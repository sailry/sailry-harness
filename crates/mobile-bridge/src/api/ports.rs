//! Mobile owns an opaque handle to the shared Client's forwarding lifecycle.
use super::*;
use sailry_client::ports::{Forwarder, State};
use tokio::{sync::watch, task::JoinHandle};

#[frb(opaque)]
pub struct Forwarding {
    local_port: u16,
    receiver: Mutex<watch::Receiver<State>>,
    stop: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Forwarding {
    pub(super) fn new(forwarder: Forwarder, stop: CancellationToken) -> Self {
        let local_port = forwarder.local_port;
        let mut receiver = forwarder.state.clone();
        receiver.mark_changed();
        let mut state = forwarder.state.clone();
        let stopping = stop.clone();
        let task = tokio::spawn(async move {
            while *state.borrow_and_update() == State::Listening {
                tokio::select! {
                    biased;
                    _ = stopping.cancelled() => break,
                    changed = state.changed() => if changed.is_err() { break; },
                }
            }
            forwarder.close().await;
        });
        Self {
            local_port,
            receiver: Mutex::new(receiver),
            stop,
            task: Mutex::new(Some(task)),
        }
    }

    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Return the current shared state, then subsequent changes without polling.
    pub async fn next(&self) -> Result<String, String> {
        let mut receiver = self.receiver.lock().await;
        receiver.changed().await.map_err(error)?;
        serde_json::to_string(&*receiver.borrow_and_update()).map_err(error)
    }

    pub async fn close(&self) {
        self.stop.cancel();
        if let Some(task) = self.task.lock().await.take() {
            let _ = task.await;
        }
    }
}

impl Drop for Forwarding {
    fn drop(&mut self) {
        // Let the task await shared cleanup even when Dart drops its handle.
        self.stop.cancel();
    }
}
