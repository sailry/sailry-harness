//! A settled boundary claims all selected inputs in the same transaction.
use super::*;
use sailry_protocol::conversation::QueuedMessage;

pub(super) async fn claim(
    db: &Connection,
    primary: &Run,
    recipient: Option<TurnId>,
) -> Result<(Vec<QueuedMessage>, Vec<Run>), Fault> {
    let boundary = recipient == Some(primary.turn);
    if !boundary && recipient.or(primary.guidance).is_none() {
        return Ok((Vec::new(), Vec::new()));
    }
    let items = super::super::queue::read(db, primary.session).await?.items;
    let start = if boundary {
        0
    } else {
        let Some(index) = items.iter().position(|item| item.turn == primary.turn) else {
            return Err(storage_error("guidance successor is no longer queued"));
        };
        index + 1
    };
    let mut inputs = Vec::new();
    let mut joined = Vec::new();
    for item in &items[start..] {
        if item.turn == primary.turn {
            continue;
        }
        if boundary && item.guidance != recipient {
            continue;
        }
        if !boundary && item.guidance.is_some() && item.guidance != recipient.or(primary.guidance) {
            break;
        }
        let mut run = get(db, item.turn).await?;
        if !boundary
            && (run.kind != RunKind::Task
                || !item.ready
                || run.notice
                || run.continuation.is_some()
                || run.status != Status::Queued
                || !super::super::queue::guidance::same_capture(db, &run, primary).await?)
        {
            break;
        }
        if boundary {
            super::super::queue::guidance::compatible(db, &run, primary).await?;
        }
        let turn = item.turn;
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

/// Whether `take` would deliver guidance to `recipient` at its next boundary.
pub(in crate::store::agent) async fn deliverable(
    db: &Connection,
    recipient: TurnId,
) -> Result<bool, Fault> {
    let run = get(db, recipient).await?;
    Ok(run.status == Status::Running
        && !super::super::queue::read(db, run.session).await?.paused
        && super::super::queue::guidance::pending(db, recipient)
            .await?
            .is_some())
}

pub(in crate::store::agent) async fn take(
    database: &mut Database,
    recipient: TurnId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Option<Vec<QueuedMessage>>, Fault> {
    if !deliverable(&database.connection, recipient).await? {
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

pub(super) async fn pending(db: &Connection, owner: TurnId) -> Result<bool, Fault> {
    let mut query = db.prepare("SELECT EXISTS(SELECT 1 FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn WHERE p.recipient=?1 AND (json_extract(a.body,'$.status')='queued' OR json_extract(a.body,'$.joined')=?1))").await.map_err(storage_error)?;
    let row = query
        .query_row([owner.to_string()])
        .await
        .map_err(storage_error)?;
    row.get(0).map_err(storage_error)
}

pub(super) async fn restore(
    db: &Connection,
    owner: TurnId,
    ready: bool,
) -> Result<Vec<Run>, Fault> {
    let mut query = db.prepare("SELECT a.body FROM agent_pending p JOIN agent_runs a ON a.turn=p.turn JOIN turns t ON t.id=p.turn WHERE json_extract(a.body,'$.joined')=?1 ORDER BY p.position,t.rowid").await.map_err(storage_error)?;
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
            "UPDATE agent_runs SET ready=?2 WHERE turn=?1",
            params![run.turn.to_string(), ready],
        )
        .await
        .map_err(storage_error)?;
        db.execute(
            "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE turn=?1",
            [run.turn.to_string()],
        )
        .await
        .map_err(storage_error)?;
    }
    if !ready && let Some(run) = restored.first() {
        super::super::queue::pause(db, run.session, true).await?;
    }
    Ok(restored)
}
