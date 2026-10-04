use super::*;
use crate::store::{Database, Ingress, Job as StoreJob};
use tokio::sync::{broadcast, oneshot};

pub(crate) struct Batch {
    pub work: Vec<jobs::Work>,
    pub pending: bool,
    pub next_ms: Option<i64>,
}

impl Ingress {
    pub(crate) async fn dispatch_tick(&self, recover: bool) -> Result<Batch, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(StoreJob::DispatchTick { recover, reply })
            .await
            .map_err(|_| crate::store::unavailable())?;
        response.await.map_err(|_| crate::store::unavailable())?
    }

    pub(crate) async fn dispatch_finished(
        &self,
        id: protocol::JobId,
        result: Result<(), Fault>,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(StoreJob::DispatchFinished { id, result, reply })
            .await
            .map_err(|_| crate::store::unavailable())?;
        response.await.map_err(|_| crate::store::unavailable())?
    }
}

pub(in crate::store) fn tick(
    database: &mut Database,
    events: &broadcast::Sender<protocol::EventEnvelope>,
    recover: bool,
) -> Result<Batch, Fault> {
    let timestamp = now();
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let observers = if recover {
        jobs::observers(&transaction, database.node)?
    } else {
        vec![]
    };
    let mut changed = schedules::due(&transaction, timestamp)?;
    let (mut work, owners) = jobs::claim(&transaction, database.node, timestamp)?;
    work.extend(observers);
    changed.extend(owners);
    changed.sort();
    changed.dedup();
    let envelope = if changed.is_empty() {
        None
    } else {
        Some(changed_event(&transaction, database.node, changed)?)
    };
    let next_ms = schedules::next(&transaction, timestamp)?;
    let pending = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM dispatch_jobs WHERE status='queued')",
            [],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    if let Some(event) = envelope {
        let _ = events.send(event);
    }
    Ok(Batch {
        work,
        pending,
        next_ms,
    })
}

pub(in crate::store) fn finish(
    database: &mut Database,
    id: protocol::JobId,
    result: Result<(), Fault>,
    events: &broadcast::Sender<protocol::EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let package = jobs::finish(&transaction, id, result)?;
    let event = changed_event(&transaction, database.node, vec![package])?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(event);
    Ok(())
}

fn changed_event(
    db: &Connection,
    node: protocol::NodeId,
    packages: Vec<String>,
) -> Result<protocol::EventEnvelope, Fault> {
    let event = protocol::Event::DispatchChanged { packages };
    db.execute("INSERT INTO events(body) VALUES(?1)", [encode(&event)?])
        .map_err(storage_error)?;
    Ok(protocol::EventEnvelope {
        node,
        cursor: db.last_insert_rowid() as u64,
        event,
    })
}
