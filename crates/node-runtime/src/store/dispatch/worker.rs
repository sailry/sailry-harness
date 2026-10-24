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

pub(in crate::store) async fn tick(
    database: &mut Database,
    events: &broadcast::Sender<protocol::EventEnvelope>,
    recover: bool,
) -> Result<Batch, Fault> {
    let timestamp = now();
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result: Result<_, Fault> = async {
        let mut transaction = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let observers = if recover {
            jobs::observers(&transaction, database.node).await?
        } else {
            vec![]
        };
        let mut changed = schedules::due(&transaction, timestamp).await?;
        let (mut work, owners) = jobs::claim(&transaction, database.node, timestamp).await?;
        work.extend(observers);
        changed.extend(owners);
        changed.sort();
        changed.dedup();
        let envelope = if changed.is_empty() {
            None
        } else {
            Some(changed_event(&transaction, database.node, changed).await?)
        };
        let next_ms = schedules::next(&transaction, timestamp).await?;
        let pending = {
            let mut statement = transaction
                .prepare("SELECT EXISTS(SELECT 1 FROM dispatch_jobs WHERE status='queued')")
                .await
                .map_err(storage_error)?;
            let row = statement.query_row(()).await.map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        drop(write.take());
        if let Some(event) = envelope {
            publication.send(events, event);
        }
        Ok(Batch {
            work,
            pending,
            next_ms,
        })
    }
    .await;
    if let Some(write) = write {
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        drop(write);
    }
    result
}

pub(in crate::store) async fn finish(
    database: &mut Database,
    id: protocol::JobId,
    result: Result<(), Fault>,
    events: &broadcast::Sender<protocol::EventEnvelope>,
) -> Result<(), Fault> {
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result: Result<_, Fault> = async {
        let mut transaction = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let package = jobs::finish(&transaction, id, result).await?;
        let event = changed_event(&transaction, database.node, vec![package]).await?;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        drop(write.take());
        publication.send(events, event);
        Ok(())
    }
    .await;
    if let Some(write) = write {
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        drop(write);
    }
    result
}

async fn changed_event(
    db: &Connection,
    node: protocol::NodeId,
    packages: Vec<String>,
) -> Result<protocol::EventEnvelope, Fault> {
    let event = protocol::Event::DispatchChanged { packages };
    super::super::database::record(db, node, vec![event])
        .await?
        .pop()
        .ok_or_else(|| storage_error("event was not recorded"))
}
