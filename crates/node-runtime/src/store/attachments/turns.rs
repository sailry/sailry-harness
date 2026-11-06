//! Frozen references retain inputs across execution, forks and history recovery.
use super::*;
use sailry_protocol::{TurnId, conversation::Run};

/// A conversation reference resolved for a running turn.
pub(crate) struct Source {
    /// The running turn's own input, including guidance joined to it.
    pub current: bool,
    pub attachment: Result<Attachment, Fault>,
    /// Why an unavailable reference was missed; set only with a fault.
    pub reason: Option<&'static str>,
}

struct Miss {
    current: bool,
    reason: &'static str,
}

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
            .ok_or_else(|| missing(id))?
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
        let mut statement = db.prepare("SELECT a.worktree, EXISTS(SELECT 1 FROM turn_attachments own JOIN agent_runs source ON source.turn=own.turn WHERE own.attachment=a.id AND (own.turn=?1 OR json_extract(source.body,'$.joined')=?1)) FROM attachments a WHERE a.id=?2 AND EXISTS(SELECT 1 FROM turn_attachments r JOIN conversation_turns h ON h.turn=r.turn JOIN turns t ON t.session=h.session JOIN agent_runs origin ON origin.turn=r.turn WHERE t.id=?1 AND r.attachment=a.id AND (r.turn=t.id OR (json_extract(origin.body,'$.joined')=t.id AND json_extract(origin.body,'$.status')='continued') OR (json_extract(origin.body,'$.status') IN ('completed','continued','failed','cancelled','interrupted') AND NOT EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=origin.turn))))").await.map_err(storage_error)?;
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
    let (worktree, current) = selected.ok_or_else(|| missing(id))?;
    let attachment = read(db, worktree.parse().map_err(storage_error)?, id).await?;
    Ok((attachment, current))
}

/// An unavailable reference keeps its fault so each caller decides whether it is fatal.
pub(in crate::store) async fn lookup(
    db: &Connection,
    node: NodeId,
    run: &Run,
    id: AttachmentId,
) -> Result<Source, Fault> {
    let fault = match source(db, run.turn, id).await {
        Ok((attachment, current)) => {
            return Ok(Source {
                current,
                attachment: Ok(attachment),
                reason: None,
            });
        }
        Err(fault) if fault.code == ErrorCode::NotFound => fault,
        Err(fault) => return Err(fault),
    };
    // An unclassified miss is treated as current input, which keeps it fatal.
    let found = miss(db, run.turn, id).await.unwrap_or(Miss {
        current: true,
        reason: "unclassified",
    });
    // Recoverable history misses are reported once per turn by the requester.
    if found.current {
        input(node, run, id, found.reason, &fault);
    }
    Ok(Source {
        current: found.current,
        attachment: Err(fault),
        reason: Some(found.reason),
    })
}

fn input(node: NodeId, run: &Run, id: AttachmentId, reason: &'static str, fault: &Fault) {
    let mut entry =
        crate::logging::Entry::new(crate::logging::Area::Run, crate::logging::Phase::Failed);
    entry.context = crate::logging::Context {
        node: Some(node),
        session: Some(run.session),
        turn: Some(run.turn),
        worktree: Some(run.worktree),
        attachment: Some(id),
        ..Default::default()
    };
    entry.operation = Some("attachment");
    entry.stage = Some("lookup");
    entry.source_kind = Some("input");
    entry.reason = Some(reason);
    entry.code = Some(fault.code);
    entry.emit_warn();
}

/// A miss whose conditions all hold on recheck was transient.
async fn miss(db: &Connection, turn: TurnId, id: AttachmentId) -> Result<Miss, Fault> {
    let mut statement = db.prepare("SELECT EXISTS(SELECT 1 FROM attachments WHERE id=?2), EXISTS(SELECT 1 FROM turn_attachments own JOIN agent_runs source ON source.turn=own.turn WHERE own.attachment=?2 AND (own.turn=?1 OR json_extract(source.body,'$.joined')=?1)), EXISTS(SELECT 1 FROM turn_attachments r JOIN conversation_turns h ON h.turn=r.turn JOIN turns t ON t.session=h.session WHERE t.id=?1 AND r.attachment=?2), EXISTS(SELECT 1 FROM turn_attachments r JOIN conversation_turns h ON h.turn=r.turn JOIN turns t ON t.session=h.session JOIN agent_pending p ON p.turn=r.turn WHERE t.id=?1 AND r.attachment=?2 AND r.turn!=t.id), EXISTS(SELECT 1 FROM turn_attachments r JOIN conversation_turns h ON h.turn=r.turn JOIN turns t ON t.session=h.session JOIN agent_runs origin ON origin.turn=r.turn WHERE t.id=?1 AND r.attachment=?2 AND r.turn!=t.id AND json_extract(origin.body,'$.status') IN ('queued','running','stopping'))").await.map_err(storage_error)?;
    let row = statement
        .query_row(params![turn.to_string(), id.to_string()])
        .await
        .map_err(storage_error)?;
    let mapped: turso::Result<(bool, bool, bool, bool, bool)> = (|| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
        ))
    })();
    let (stored, current, member, pending, active) = mapped.map_err(storage_error)?;
    let reason = if !stored {
        "missing_record"
    } else if !member {
        "outside_history"
    } else if pending {
        "pending_input"
    } else if active {
        "active_origin"
    } else {
        "visible_on_recheck"
    };
    Ok(Miss { current, reason })
}

#[cfg(test)]
mod tests;
