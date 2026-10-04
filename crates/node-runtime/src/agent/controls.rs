use sailry_link::CancellationToken;
use sailry_protocol::TurnId;
use std::{collections::BTreeMap, sync::Mutex};
use tokio::sync::Notify;

struct Active {
    stop: CancellationToken,
    operations: usize,
    media: Option<super::media::Binding>,
    finishing: bool,
}

#[derive(Default)]
pub(crate) struct Controls {
    pub(super) mcp: super::mcp::Pool,
    pub(crate) wake: Notify,
    pub(crate) stopped: CancellationToken,
    active: Mutex<BTreeMap<TurnId, Active>>,
    changed: Notify,
}

impl Controls {
    pub(crate) async fn close(&self) -> Result<(), sailry_protocol::Fault> {
        self.mcp.close().await
    }
    pub fn register(&self, turn: TurnId, stop: CancellationToken) {
        self.active.lock().unwrap().insert(
            turn,
            Active {
                stop,
                operations: 0,
                media: None,
                finishing: false,
            },
        );
    }

    pub fn bind_media(&self, turn: TurnId, binding: super::media::Binding) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.media.get_or_insert(binding);
        }
    }

    pub fn media(&self, turn: TurnId) -> Option<super::media::Binding> {
        self.active.lock().unwrap().get(&turn)?.media.clone()
    }

    pub fn cancel(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get(&turn) {
            active.stop.cancel();
        }
    }

    pub fn stop(&self) {
        self.stopped.cancel();
        for active in self.active.lock().unwrap().values() {
            active.stop.cancel();
        }
    }

    pub fn begin_operation(&self, turn: TurnId) -> Option<CancellationToken> {
        let mut runs = self.active.lock().unwrap();
        let active = runs.get_mut(&turn)?;
        if active.finishing {
            return None;
        }
        active.operations += 1;
        Some(active.stop.clone())
    }

    pub fn finish_operation(&self, turn: TurnId) {
        if let Some(active) = self.active.lock().unwrap().get_mut(&turn) {
            active.operations -= 1;
        }
        self.changed.notify_waiters();
    }

    pub async fn drain(&self, turn: TurnId) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut runs = self.active.lock().unwrap();
                let Some(active) = runs.get_mut(&turn) else {
                    return;
                };
                // Sealing and registration use one lock, so no operation can enter
                // between the empty check and the durable turn completion.
                active.finishing = true;
                if active.operations == 0 {
                    return;
                }
            }
            notified.await;
        }
    }

    pub fn finish(&self, turn: TurnId) {
        self.active.lock().unwrap().remove(&turn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn seals_operations_before_completion() {
        let controls = Controls::default();
        let turn = TurnId::new();
        controls.register(turn, CancellationToken::new());
        let token = controls.begin_operation(turn).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), controls.drain(turn))
                .await
                .is_err()
        );
        assert!(controls.begin_operation(turn).is_none());
        assert!(!token.is_cancelled());
        controls.finish_operation(turn);
        controls.drain(turn).await;
        controls.finish(turn);
        assert!(controls.begin_operation(turn).is_none());
    }
}
