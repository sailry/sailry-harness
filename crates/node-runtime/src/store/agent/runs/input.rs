//! One immutable execution capture, projected without creating canonical history.
use super::*;
use sailry_protocol::conversation::{Input, SentInput};

pub(in crate::store::agent) async fn capture(
    db: &Connection,
    turn: &QueuedTurn,
    automatic: bool,
    message: &Input,
) -> Result<(), Fault> {
    if turn.kind != RunKind::Task || automatic {
        return Ok(());
    }
    db.execute(
        "UPDATE agent_runs SET input=?2 WHERE turn=?1 AND input IS NULL",
        params![turn.id.to_string(), encode(message)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store::agent) async fn hydrate(db: &Connection, mut run: Run) -> Result<Run, Fault> {
    run.input = None;
    if run.kind != RunKind::Task || run.continuation.is_some() {
        return Ok(run);
    }
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare(
                "SELECT input FROM agent_runs WHERE turn=?1 AND NOT EXISTS(
                SELECT 1 FROM agent_events WHERE turn=?1 AND json_extract(body,'$.author')='user')",
            )
            .await
            .map_err(storage_error)?;
        match statement.query_row([run.turn.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    }
    .flatten();
    if let Some(body) = body {
        let message: Input = serde_json::from_slice(&body).map_err(storage_error)?;
        let attachments = super::super::super::attachments::turns::list(db, run.turn).await?;
        run.input = Some(SentInput {
            message,
            attachments,
        });
    }
    Ok(run)
}
