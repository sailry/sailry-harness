//! Frozen references retain inputs across execution, forks and history recovery.
use super::*;
use sailry_protocol::TurnId;

pub(in crate::store) fn bind(
    db: &Connection,
    turn: TurnId,
    ids: &[AttachmentId],
) -> Result<(), Fault> {
    clear(db, turn)?;
    if ids.is_empty() {
        return Ok(());
    }
    let worktree: String = db
        .query_row(
            "SELECT s.worktree FROM turns t JOIN sessions s ON s.id=t.session WHERE t.id=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let worktree = worktree.parse().map_err(storage_error)?;
    for (position, id) in ids.iter().enumerate() {
        read(db, worktree, *id)?;
        db.execute(
            "INSERT INTO turn_attachments(turn,attachment,position) VALUES(?1,?2,?3)",
            params![turn.to_string(), id.to_string(), position as i64],
        )
        .map_err(storage_error)?;
    }
    Ok(())
}

pub(in crate::store) fn clear(db: &Connection, turn: TurnId) -> Result<(), Fault> {
    db.execute(
        "DELETE FROM turn_attachments WHERE turn=?1",
        [turn.to_string()],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) fn list(db: &Connection, turn: TurnId) -> Result<Vec<Attachment>, Fault> {
    let mut query = db.prepare("SELECT a.id,a.worktree FROM turn_attachments r JOIN attachments a ON a.id=r.attachment WHERE r.turn=?1 ORDER BY r.position").map_err(storage_error)?;
    query
        .query_map([turn.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (id, worktree) = row.map_err(storage_error)?;
            read(
                db,
                worktree.parse().map_err(storage_error)?,
                id.parse().map_err(storage_error)?,
            )
        })
        .collect()
}

/// Canonical events keep their original turn when a rewind moves their visible history.
pub(in crate::store) fn get(
    db: &Connection,
    turn: TurnId,
    id: AttachmentId,
) -> Result<Attachment, Fault> {
    let worktree: Option<String> = db.query_row(
        "SELECT a.worktree FROM turn_attachments r JOIN attachments a ON a.id=r.attachment WHERE r.turn=?1 AND r.attachment=?2",
        params![turn.to_string(), id.to_string()], |row| row.get(0),
    ).optional().map_err(storage_error)?;
    read(
        db,
        worktree
            .ok_or_else(missing)?
            .parse()
            .map_err(storage_error)?,
        id,
    )
}

/// Resolves only references in the executing turn's visible conversation history.
pub(in crate::store) fn resolve(
    db: &Connection,
    turn: TurnId,
    id: AttachmentId,
) -> Result<Attachment, Fault> {
    let worktree: Option<String> = db.query_row(
        "SELECT a.worktree FROM attachments a WHERE a.id=?2 AND EXISTS(SELECT 1 FROM turn_attachments r JOIN conversation_turns h ON h.turn=r.turn JOIN turns t ON t.session=h.session WHERE t.id=?1 AND r.attachment=a.id)",
        params![turn.to_string(), id.to_string()], |row| row.get(0),
    ).optional().map_err(storage_error)?;
    read(
        db,
        worktree
            .ok_or_else(missing)?
            .parse()
            .map_err(storage_error)?,
        id,
    )
}

#[cfg(test)]
mod tests;
