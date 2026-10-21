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

impl Ingress {
    pub(crate) async fn hold_approval_commit(
        &self,
        after_claim: bool,
        fail: bool,
    ) -> Result<(Arc<Notify>, mpsc::Sender<()>), Fault> {
        let entered = Arc::new(Notify::new());
        let (release, receiver) = mpsc::channel();
        let (reply, ready) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::ApprovalCommitFixture {
                gate: Commit {
                    after_claim,
                    fail,
                    entered: entered.clone(),
                    release: receiver,
                },
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

pub(in crate::store) fn wait(gate: &mut Option<Commit>, after_claim: bool, db: &Connection) {
    if !gate
        .as_ref()
        .is_some_and(|gate| gate.after_claim == after_claim)
    {
        return;
    }
    let gate = gate.take().unwrap();
    gate.entered.notify_one();
    gate.release
        .recv()
        .expect("approval commit barrier was dropped");
    if gate.fail {
        let mut reject = true;
        db.commit_hook(Some(move || std::mem::take(&mut reject)))
            .unwrap();
    }
}
