//! The store owns each main session's child execution slots and FIFO order.
use super::*;

#[cfg(test)]
mod tests;

pub(in crate::store::agent) async fn claim(
    database: &mut Database,
    turn: TurnId,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Status, Fault> {
    let mut run = runs::get(&database.connection, turn).await?;
    let association = origin(&database.connection, turn)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::WrongTarget, "turn is not a delegated child"))?;
    if run.status != Status::Queued {
        return Ok(run.status);
    }
    // Guidance retains the original parent turn, and later parent turns share
    // its session identity. Neither a turn boundary nor another main session
    // changes this execution's concurrency owner.
    let owner = runs::get(&database.connection, association.turn)
        .await?
        .session;
    let tx = database
        .connection
        .unchecked_transaction()
        .await
        .map_err(storage_error)?;
    let limit = crate::store::plugins::policy::child_limit(&tx).await?;
    let active: i64 = {
        let mut statement = tx.prepare("SELECT count(*) FROM agent_runs a JOIN agent_delegations d ON d.turn=a.turn JOIN turns parent ON parent.id=d.parent_turn
         WHERE parent.session=?1 AND json_extract(a.body,'$.status') IN ('running','stopping')").await.map_err(storage_error)?;
        let row = statement
            .query_row([owner.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
    let active = u64::try_from(active).map_err(storage_error)?;
    if limit.is_some_and(|limit| active >= limit) {
        return Ok(Status::Queued);
    }
    // Unselected guidance still belongs to its existing queue owner. It must
    // not block unrelated children while its preceding execution is running.
    let next: Option<String> = {
        let mut statement = tx.prepare("SELECT a.turn FROM agent_runs a JOIN agent_delegations d ON d.turn=a.turn JOIN turns t ON t.id=a.turn JOIN turns parent ON parent.id=d.parent_turn
         WHERE parent.session=?1 AND a.ready=1 AND json_extract(a.body,'$.status')='queued'
           AND NOT EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=a.turn)
           AND NOT EXISTS(SELECT 1 FROM agent_runs running JOIN turns current ON current.id=running.turn WHERE current.session=t.session AND json_extract(running.body,'$.status') IN ('running','stopping'))
         ORDER BY t.rowid LIMIT 1").await.map_err(storage_error)?;
        match statement.query_row([owner.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    if next != Some(turn.to_string()) {
        return Ok(Status::Queued);
    }
    run.status = Status::Running;
    run.started_ms = Some(chrono::Utc::now().timestamp_millis());
    runs::put(&tx, &run).await?;
    let child = super::super::children::get(&tx, &run).await?;
    let envelope = super::super::changed(&tx, database.node, run.session).await?;
    tx.commit().await.map_err(storage_error)?;
    let _ = events.send(envelope);
    super::super::feed::run(database, run).await;
    if let Some(child) = child {
        database
            .feeds
            .publish(database.node, child.origin.session, Change::Child(child));
    }
    Ok(Status::Running)
}

pub(in crate::store::agent) async fn finish_queued(
    database: &mut Database,
    turn: TurnId,
    status: Status,
    error: Option<Fault>,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Status, Fault> {
    let run = runs::get(&database.connection, turn).await?;
    if origin(&database.connection, turn).await?.is_none()
        || !matches!(
            status,
            Status::Cancelled | Status::Interrupted | Status::Failed
        )
    {
        return Err(Fault::invalid("invalid queued child completion"));
    }
    if run.finished_ms.is_some() {
        return Ok(run.status);
    }
    if run.started_ms.is_some() || !matches!(run.status, Status::Queued | Status::Cancelled) {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "child execution is no longer queued",
        ));
    }
    let error = if status == Status::Interrupted && error.is_none() {
        Some(Fault::new(
            ErrorCode::OutcomeUnknown,
            "Node stopped execution; calls were not replayed",
        ))
    } else {
        error
    };
    runs::finish_with(database, turn, status, error, None, events).await
}
