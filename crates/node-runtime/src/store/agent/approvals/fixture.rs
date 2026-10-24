//! Isolated barriers around the real approval commit path.
use super::*;
use std::sync::{Arc, mpsc};
use tokio::sync::Notify;

pub(in crate::store) struct Commit {
    after_claim: bool,
    fail: bool,
    entered: Arc<Notify>,
    release: mpsc::Receiver<()>,
}

impl Commit {
    pub(in crate::store) fn new(
        after_claim: bool,
        fail: bool,
    ) -> (Self, Arc<Notify>, mpsc::Sender<()>) {
        let entered = Arc::new(Notify::new());
        let (release, receiver) = mpsc::channel();
        (
            Self {
                after_claim,
                fail,
                entered: entered.clone(),
                release: receiver,
            },
            entered,
            release,
        )
    }
}

impl Ingress {
    pub(crate) async fn hold_approval_commit(
        &self,
        after_claim: bool,
        fail: bool,
    ) -> Result<(Arc<Notify>, mpsc::Sender<()>), Fault> {
        let (gate, entered, release) = Commit::new(after_claim, fail);
        let (reply, ready) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ApprovalCommitFixture {
                gate,
                reply,
            })))
            .await
            .map_err(|_| super::super::super::unavailable())?;
        ready
            .await
            .map_err(|_| super::super::super::unavailable())?;
        Ok((entered, release))
    }
}

pub(in crate::store) async fn wait(
    gate: &std::sync::Mutex<Option<Commit>>,
    after_claim: bool,
    db: &Connection,
) {
    let gate = {
        let mut gate = gate.lock().unwrap();
        if !gate
            .as_ref()
            .is_some_and(|gate| gate.after_claim == after_claim)
        {
            return;
        }
        gate.take().unwrap()
    };
    gate.entered.notify_one();
    tokio::task::spawn_blocking(move || gate.release.recv())
        .await
        .expect("approval barrier worker failed")
        .expect("approval commit barrier was dropped");
    if gate.fail {
        // Reject the production COMMIT with a real, already rolled-back transaction.
        // This fixture does not simulate an uncertain I/O failure.
        db.execute("ROLLBACK", ())
            .await
            .expect("approval fixture rollback failed");
    }
}
