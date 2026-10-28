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
    let (body, guided): (Option<Vec<u8>>, bool) = {
        let mut statement = db
            .prepare(
                "SELECT input,EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=?1 AND p.recipient IS NOT NULL)
                FROM agent_runs WHERE turn=?1 AND NOT EXISTS(
                SELECT 1 FROM agent_events WHERE turn=?1 AND json_extract(body,'$.provenance.author')='user')",
            )
            .await
            .map_err(storage_error)?;
        match statement.query_row([run.turn.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                mapped.map_err(storage_error)?
            }
            Err(turso::Error::QueryReturnedNoRows) => (None, false),
            Err(error) => return Err(storage_error(error)),
        }
    };
    let message = if let Some(body) = body {
        Some(serde_json::from_slice::<Input>(&body).map_err(storage_error)?)
    } else if run.status == Status::Queued && guided {
        Some(super::super::queue::message(db, run.turn).await?.message)
    } else {
        None
    };
    if let Some(message) = message {
        let attachments = super::super::super::attachments::turns::list(db, run.turn).await?;
        run.input = Some(SentInput {
            message,
            attachments,
        });
    }
    Ok(run)
}
