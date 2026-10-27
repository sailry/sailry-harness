//! A settled boundary claims all selected inputs in the same transaction.
use super::*;
use sailry_protocol::conversation::QueuedMessage;

pub(super) async fn claim(
    db: &Connection,
    primary: &Run,
    recipient: Option<TurnId>,
) -> Result<(Vec<QueuedMessage>, Vec<Run>), Fault> {
    let Some(recipient) = recipient else {
        return Ok((Vec::new(), Vec::new()));
    };
    let turns: Vec<_> = super::super::queue::read(db, primary.session)
        .await?
        .items
        .into_iter()
        .filter(|item| item.guidance == Some(recipient) && item.turn != primary.turn)
        .map(|item| item.turn)
        .collect();
    let mut inputs = Vec::with_capacity(turns.len());
    let mut joined = Vec::with_capacity(turns.len());
    for turn in turns {
        let mut run = get(db, turn).await?;
        super::super::queue::guidance::compatible(db, &run, primary).await?;
        let input = super::super::queue::message(db, turn).await?;
        capture(db, &input.turn, false, &input.message).await?;
        run.joined = Some(primary.turn);
        run.status = Status::Continued;
        run.finished_ms = Some(chrono::Utc::now().timestamp_millis());
        put(db, &run).await?;
        // The pending ledger retains edits and caller identity until canonical
        // input is appended. A preparation failure can then restore the input.
        inputs.push(input);
        joined.push(run);
    }
    Ok((inputs, joined))
}

pub(in crate::store::agent) async fn take(
    database: &mut Database,
    recipient: TurnId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Vec<QueuedMessage>>, Fault> {
    let run = get(&database.connection, recipient).await?;
    if run.status != Status::Running
        || super::super::queue::read(&database.connection, run.session)
            .await?
            .paused
        || super::super::queue::guidance::pending(&database.connection, recipient)
            .await?
            .is_none()
    {
        return Ok(None);
    }
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result = async {
        let mut transaction = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let run = get(&transaction, recipient).await?;
        if run.status != Status::Running
            || super::super::queue::read(&transaction, run.session)
                .await?
                .paused
        {
            return Ok(None);
        }
        let (inputs, joined) = claim(&transaction, &run, Some(recipient)).await?;
        if inputs.is_empty() {
            return Ok(None);
        }
        super::super::queue::bump(&transaction, run.session).await?;
        let queue = super::super::queue::read(&transaction, run.session).await?;
        let envelope = super::super::changed(&transaction, database.node, run.session).await?;
        let mut children = Vec::with_capacity(joined.len());
        for run in joined {
            let child = super::super::children::get(&transaction, &run).await?;
            children.push((run, child));
        }
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        drop(write.take());
        publication.send(events, envelope);
        database
            .feeds
            .publish(database.node, run.session, Change::Queue(queue));
        for (run, child) in children {
            super::super::feed::run(database, run).await;
            if let Some(child) = child {
                database
                    .feeds
                    .publish(database.node, child.origin.session, Change::Child(child));
            }
        }
        Ok(Some(inputs))
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

pub(super) async fn restore(db: &Connection, owner: TurnId) -> Result<Vec<Run>, Fault> {
    let mut query = db.prepare("SELECT a.body FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn WHERE json_extract(a.body,'$.joined')=?1").await.map_err(storage_error)?;
    let mut rows = query
        .query([owner.to_string()])
        .await
        .map_err(storage_error)?;
    let mut restored = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let body: Vec<u8> = row.get(0).map_err(storage_error)?;
        restored.push(serde_json::from_slice::<Run>(&body).map_err(storage_error)?);
    }
    drop(rows);
    drop(query);
    for run in &mut restored {
        run.joined = None;
        run.status = Status::Queued;
        run.finished_ms = None;
        put(db, run).await?;
        db.execute(
            "UPDATE agent_runs SET ready=0 WHERE turn=?1",
            [run.turn.to_string()],
        )
        .await
        .map_err(storage_error)?;
        db.execute(
            "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE turn=?1",
            [run.turn.to_string()],
        )
        .await
        .map_err(storage_error)?;
        super::super::queue::pause(db, run.session, true).await?;
    }
    Ok(restored)
}
