//! Barrier after a real durable commit and before ordered publication.
use crate::store::{Ingress, Job, agent::Operation};
use sailry_protocol::Fault;
use std::sync::{Arc, Mutex};
use tokio::sync::{Notify, oneshot};

pub(in crate::store) struct Gate {
    entered: Arc<Notify>,
    release: oneshot::Receiver<()>,
}

impl Ingress {
    pub(crate) async fn hold_publication(
        &self,
    ) -> Result<(Arc<Notify>, oneshot::Sender<()>), Fault> {
        let entered = Arc::new(Notify::new());
        let (release, receiver) = oneshot::channel();
        let gate = Gate {
            entered: entered.clone(),
            release: receiver,
        };
        let (reply, ready) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::PublicationFixture {
            gate,
            reply,
        })))
        .await
        .map_err(|_| crate::store::unavailable())?;
        ready.await.map_err(|_| crate::store::unavailable())?;
        Ok((entered, release))
    }
}

pub(in crate::store) async fn wait(gate: &Mutex<Option<Gate>>) {
    let gate = gate.lock().unwrap().take();
    if let Some(gate) = gate {
        gate.entered.notify_one();
        // Dropping the test controller releases the owned operation too.
        let _ = gate.release.await;
    }
}
