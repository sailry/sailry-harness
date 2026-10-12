//! One immutable claim capture, projected without creating canonical history.
use super::*;
use sailry_protocol::conversation::{Input, SentInput};

pub(super) fn capture(db: &Connection, invocation: &Invocation) -> Result<(), Fault> {
    if invocation.turn.kind != RunKind::Task || invocation.automatic || invocation.child.is_some() {
        return Ok(());
    }
    db.execute(
        "UPDATE agent_runs SET input=?2 WHERE turn=?1 AND input IS NULL",
        params![invocation.turn.id.to_string(), encode(&invocation.message)?],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store::agent) fn hydrate(db: &Connection, mut run: Run) -> Result<Run, Fault> {
    run.input = None;
    if run.kind != RunKind::Task || run.continuation.is_some() {
        return Ok(run);
    }
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT input FROM agent_runs WHERE turn=?1 AND NOT EXISTS(
                SELECT 1 FROM agent_events WHERE turn=?1 AND json_extract(body,'$.author')='user')",
            [run.turn.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?
        .flatten();
    if let Some(body) = body {
        let message: Input = serde_json::from_slice(&body).map_err(storage_error)?;
        let attachments = super::super::super::attachments::turns::list(db, run.turn)?;
        run.input = Some(SentInput {
            message,
            attachments,
        });
    }
    Ok(run)
}
