//! Frozen references retain inputs across execution, forks and history recovery.
use super::*;
use sailry_protocol::TurnId;

pub(in crate::store) async fn bind(
    db: &Connection,
    turn: TurnId,
    ids: &[AttachmentId],
) -> Result<(), Fault> {
    clear(db, turn).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let worktree: String = {
        let mut statement = db
            .prepare(
                "SELECT s.worktree FROM turns t JOIN sessions s ON s.id=t.session WHERE t.id=?1",
            )
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    let worktree = worktree.parse().map_err(storage_error)?;
    for (position, id) in ids.iter().enumerate() {
        read(db, worktree, *id).await?;
        db.execute(
            "INSERT INTO turn_attachments(turn,attachment,position) VALUES(?1,?2,?3)",
            params![turn.to_string(), id.to_string(), position as i64],
        )
        .await
        .map_err(storage_error)?;
    }
    Ok(())
}

pub(in crate::store) async fn clear(db: &Connection, turn: TurnId) -> Result<(), Fault> {
    db.execute(
        "DELETE FROM turn_attachments WHERE turn=?1",
        [turn.to_string()],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn list(
    db: &Connection,
    turn: TurnId,
) -> Result<Vec<Attachment>, Fault> {
    let mut rows = db.query("SELECT a.id,a.worktree FROM turn_attachments r JOIN attachments a ON a.id=r.attachment WHERE r.turn=?1 ORDER BY r.position", [turn.to_string()]).await.map_err(storage_error)?;
    let mut selected = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let id: String = row.get(0).map_err(storage_error)?;
        let worktree: String = row.get(1).map_err(storage_error)?;
        selected.push(
            read(
                db,
                worktree.parse().map_err(storage_error)?,
                id.parse().map_err(storage_error)?,
            )
            .await?,
        );
    }
    Ok(selected)
}

/// Canonical events keep their original turn when a rewind moves their visible history.
pub(in crate::store) async fn get(
    db: &Connection,
    turn: TurnId,
    id: AttachmentId,
) -> Result<Attachment, Fault> {
    let worktree: Option<String> = {
        let mut statement = db.prepare("SELECT a.worktree FROM turn_attachments r JOIN attachments a ON a.id=r.attachment WHERE r.turn=?1 AND r.attachment=?2").await.map_err(storage_error)?;
        match statement
            .query_row(params![turn.to_string(), id.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    read(
        db,
        worktree
            .ok_or_else(missing)?
            .parse()
            .map_err(storage_error)?,
        id,
    )
    .await
}

/// Resolves only references in the executing turn's visible conversation history.
pub(in crate::store) async fn resolve(
    db: &Connection,
    turn: TurnId,
    id: AttachmentId,
) -> Result<Attachment, Fault> {
    source(db, turn, id).await.map(|(attachment, _)| attachment)
}

/// Fresh inputs include guidance joined to this same running task.
pub(in crate::store) async fn source(
    db: &Connection,
    turn: TurnId,
    id: AttachmentId,
) -> Result<(Attachment, bool), Fault> {
    let selected: Option<(String, bool)> = {
        let mut statement = db.prepare("SELECT a.worktree, EXISTS(SELECT 1 FROM turn_attachments own JOIN agent_runs source ON source.turn=own.turn WHERE own.attachment=a.id AND (own.turn=?1 OR json_extract(source.body,'$.joined')=?1)) FROM attachments a WHERE a.id=?2 AND EXISTS(SELECT 1 FROM turn_attachments r JOIN conversation_turns h ON h.turn=r.turn JOIN turns t ON t.session=h.session JOIN agent_runs origin ON origin.turn=r.turn WHERE t.id=?1 AND r.attachment=a.id AND (r.turn=t.id OR (json_extract(origin.body,'$.joined')=t.id AND json_extract(origin.body,'$.status')='continued') OR (json_extract(origin.body,'$.status') IN ('completed','continued','failed','cancelled') AND NOT EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=origin.turn))))").await.map_err(storage_error)?;
        match statement
            .query_row(params![turn.to_string(), id.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> = (|| Ok((row.get(0)?, row.get(1)?)))();
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let (worktree, current) = selected.ok_or_else(missing)?;
    let attachment = read(db, worktree.parse().map_err(storage_error)?, id).await?;
    Ok((attachment, current))
}

#[cfg(test)]
mod tests;
