use super::*;

pub(in crate::store) fn list(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    before: Option<CheckpointId>,
    limit: u16,
) -> Result<checkpoint::Page, Fault> {
    runs::visible(db, session, turn)?;
    if !(1..=100).contains(&limit) {
        return Err(invalid(
            "file checkpoint page limit must be between 1 and 100",
        ));
    }
    let boundary = before
        .map(|id| {
            db.query_row(
                "SELECT rowid FROM file_checkpoints WHERE id=?1 AND turn=?2",
                params![id.to_string(), turn.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(|| {
                Fault::new(
                    ErrorCode::WrongTarget,
                    "file checkpoint cursor belongs to another turn",
                )
            })
        })
        .transpose()?;
    let mut query = db.prepare("SELECT id FROM file_checkpoints WHERE turn=?1 AND rowid<?2 ORDER BY rowid DESC LIMIT ?3")
        .map_err(storage_error)?;
    let ids = query
        .query_map(
            params![
                turn.to_string(),
                boundary.unwrap_or(i64::MAX),
                i64::from(limit) + 1
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(storage_error)?
        .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
        .collect::<Result<Vec<CheckpointId>, Fault>>()?;
    let more = ids.len() > usize::from(limit);
    let files = ids
        .into_iter()
        .take(usize::from(limit))
        .map(|id| file(db, session, id))
        .collect::<Result<Vec<_>, Fault>>()?;
    let next = more.then(|| files.last().unwrap().id);
    Ok(checkpoint::Page {
        session,
        turn,
        files,
        next,
    })
}

pub(in crate::store) fn read(
    db: &Connection,
    session: SessionId,
    id: CheckpointId,
) -> Result<checkpoint::Content, Fault> {
    let file = file(db, session, id)?;
    let (before, after): (Option<String>, String) = db
        .query_row(
            "SELECT before_text,after_text FROM file_checkpoints WHERE id=?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(storage_error)?;
    if before.as_deref().map(version) != file.before || version(&after) != file.after {
        return Err(storage_error(
            "checkpoint content does not match its revision",
        ));
    }
    Ok(checkpoint::Content {
        session,
        file,
        before,
        after,
    })
}

fn file(db: &Connection, session: SessionId, id: CheckpointId) -> Result<checkpoint::File, Fault> {
    let row = db.query_row(
        "SELECT c.body,a.body,t.caller,a.request FROM file_checkpoints c JOIN agent_approvals a ON a.id=c.approval
         JOIN turns t ON t.id=c.turn WHERE c.id=?1", [id.to_string()],
        |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, Vec<u8>>(2)?, row.get::<_, String>(3)?)),
    ).optional().map_err(storage_error)?;
    let (body, approval, caller, request) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "file checkpoint does not exist"))?;
    let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    let approval: Approval = serde_json::from_slice(&approval).map_err(storage_error)?;
    runs::visible(db, session, approval.turn)?;
    let caller = NodeId(
        caller
            .try_into()
            .map_err(|_| storage_error("invalid checkpoint caller"))?,
    );
    let outcome = super::super::super::outcomes::inspect(
        db,
        caller,
        request.parse().map_err(storage_error)?,
        &record.digest,
    )?;
    Ok(checkpoint::File {
        id,
        turn: approval.turn,
        worktree: record.worktree,
        path: record.path,
        before: record.before,
        after: record.after,
        outcome,
    })
}
