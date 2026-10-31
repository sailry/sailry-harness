//! Holds a derived snapshot without blocking the writer or ordered canonical events.
use super::*;
use tokio::sync::{Notify, oneshot};

pub(super) struct Gate {
    entered: Arc<Notify>,
    release: oneshot::Receiver<()>,
}

pub(super) fn hold(cache: &Cache, session: SessionId) -> (Arc<Notify>, oneshot::Sender<()>) {
    let entered = Arc::new(Notify::new());
    let (release, receiver) = oneshot::channel();
    assert!(
        cache
            .publication
            .gates
            .lock()
            .unwrap()
            .insert(
                session,
                Gate {
                    entered: entered.clone(),
                    release: receiver,
                }
            )
            .is_none(),
        "one statistics gate per session"
    );
    (entered, release)
}

pub(super) async fn wait(cache: &Cache, session: SessionId) {
    let gate = cache.publication.gates.lock().unwrap().remove(&session);
    if let Some(gate) = gate {
        gate.entered.notify_one();
        let _ = gate.release.await;
    }
}

impl crate::Node {
    /// Holds one committed statistics snapshot in an isolated integration fixture.
    pub fn hold_statistics_fixture(
        &self,
        session: SessionId,
    ) -> Result<(Arc<Notify>, oneshot::Sender<()>), Fault> {
        let storage = self
            .ingress
            .storage
            .get()
            .ok_or_else(crate::store::unavailable)?;
        Ok(hold(&storage.statistics, session))
    }
}
