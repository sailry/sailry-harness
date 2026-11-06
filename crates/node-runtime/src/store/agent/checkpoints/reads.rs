use super::*;

pub(in crate::store) async fn list(
    db: &Connection,
    session: SessionId,
    turn: TurnId,
    before: Option<CheckpointId>,
    limit: u16,
) -> Result<checkpoint::Page, Fault> {
    runs::visible(db, session, turn).await?;
    if !(1..=100).contains(&limit) {
        return Err(sailry_protocol::Fault::invalid(
            "file checkpoint page limit must be between 1 and 100",
        ));
    }
    // Newest first: a change set lists later entries before its first file.
    // A cursor inside one continues with that set's earlier files.
    let mut ids = Vec::new();
    let boundary = match before {
        Some(id) => {
            let (row, parent, position) = cursor(db, turn, id).await?.ok_or_else(|| {
                Fault::new(
                    ErrorCode::WrongTarget,
                    "file checkpoint cursor belongs to another turn",
                )
            })?;
            if let Some(position) = position {
                ids.extend(
                    entries(db, &parent)
                        .await?
                        .into_iter()
                        .filter(|(entry, _)| *entry < position)
                        .map(|(_, id)| id),
                );
                ids.push(parent.parse().map_err(storage_error)?);
            }
            row
        }
        None => i64::MAX,
    };
    let mut query = db.prepare("SELECT id FROM file_checkpoints WHERE turn=?1 AND rowid<?2 ORDER BY rowid DESC LIMIT ?3").await
        .map_err(storage_error)?;
    let parents = {
        let mut rows = query
            .query(params![turn.to_string(), boundary, i64::from(limit) + 1])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            values.push(row.get::<String>(0).map_err(storage_error)?);
        }
        values
    };
    for parent in parents {
        if ids.len() > usize::from(limit) {
            break;
        }
        ids.extend(entries(db, &parent).await?.into_iter().map(|(_, id)| id));
        ids.push(parent.parse().map_err(storage_error)?);
    }
    let more = ids.len() > usize::from(limit);
    let files = {
        let mut selected = Vec::new();
        for value in ids.into_iter().take(usize::from(limit)) {
            selected.push(
                async {
                    let id: CheckpointId = value;
                    file(db, session, id).await
                }
                .await?,
            );
        }
        Ok::<_, Fault>(selected)
    }?;
    let next = more.then(|| files.last().unwrap().id);
    Ok(checkpoint::Page {
        session,
        turn,
        files,
        next,
    })
}

pub(in crate::store) async fn read(
    db: &Connection,
    session: SessionId,
    id: CheckpointId,
) -> Result<checkpoint::Content, Fault> {
    let file = file(db, session, id).await?;
    let size = file
        .after
        .size
        .saturating_add(file.before.as_ref().map_or(0, |value| value.size));
    // JSON may expand a control byte to six bytes; reserve space for the envelope.
    if size > (MAX_FRAME_BYTES / 8) as u64 {
        return Err(Fault::invalid(
            "checkpoint content exceeds the response limit; use the turn diff or restore the checkpoint",
        ));
    }
    content(db, session, id).await
}

/// Confirmed file versions saved before one approved call dispatched.
pub(in crate::store) struct Saved {
    pub path: String,
    pub before: Option<String>,
    pub after: String,
}

/// The snapshot of a turn's call, if its mutation was approved and prepared.
pub(in crate::store) async fn saved(
    db: &Connection,
    turn: TurnId,
    call: &str,
) -> Result<Option<Saved>, Fault> {
    let mut statement = db
        .prepare(
            "SELECT c.body,c.before_text,c.after_text FROM file_checkpoints c
             JOIN agent_approvals a ON a.id=c.approval
             WHERE a.turn=?1 AND a.call=?2",
        )
        .await
        .map_err(storage_error)?;
    let row = match statement.query_row(params![turn.to_string(), call]).await {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let mapped: turso::Result<_> = (|row: &turso::Row| {
        Ok((
            row.get::<Vec<u8>>(0)?,
            row.get::<Option<String>>(1)?,
            row.get::<String>(2)?,
        ))
    })(&row);
    let (body, before, after) = mapped.map_err(storage_error)?;
    let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    // One file cannot stand for a whole change set; its declared result stays visible.
    if record.set {
        return Ok(None);
    }
    Ok(Some(Saved {
        path: record.path,
        before,
        after,
    }))
}

pub(super) async fn content(
    db: &Connection,
    session: SessionId,
    id: CheckpointId,
) -> Result<checkpoint::Content, Fault> {
    let file = file(db, session, id).await?;
    let mut texts: Option<(Option<String>, String)> = None;
    for table in ["file_checkpoints", "file_checkpoint_entries"] {
        let mut statement = db
            .prepare(format!(
                "SELECT before_text,after_text FROM {table} WHERE id=?1"
            ))
            .await
            .map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                texts = Some(mapped.map_err(storage_error)?);
                break;
            }
            Err(turso::Error::QueryReturnedNoRows) => {}
            Err(error) => return Err(storage_error(error)),
        }
    }
    let (before, after) =
        texts.ok_or_else(|| Fault::new(ErrorCode::NotFound, "file checkpoint does not exist"))?;
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

async fn file(
    db: &Connection,
    session: SessionId,
    id: CheckpointId,
) -> Result<checkpoint::File, Fault> {
    let mut row = None;
    // Later change-set files share the approval and turn of their set's first file.
    for query in [
        "SELECT c.body,a.body,t.caller,a.request FROM file_checkpoints c JOIN agent_approvals a ON a.id=c.approval
         JOIN turns t ON t.id=c.turn WHERE c.id=?1",
        "SELECT e.body,a.body,t.caller,a.request FROM file_checkpoint_entries e JOIN file_checkpoints c ON c.id=e.checkpoint
         JOIN agent_approvals a ON a.id=c.approval JOIN turns t ON t.id=c.turn WHERE e.id=?1",
    ] {
        let mut statement = db.prepare(query).await.map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(found) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| {
                    Ok((
                        row.get::<Vec<u8>>(0)?,
                        row.get::<Vec<u8>>(1)?,
                        row.get::<Vec<u8>>(2)?,
                        row.get::<String>(3)?,
                    ))
                })(&found);
                row = Some(mapped.map_err(storage_error)?);
                break;
            }
            Err(turso::Error::QueryReturnedNoRows) => {}
            Err(error) => return Err(storage_error(error)),
        }
    }
    let (body, approval, caller, request) =
        row.ok_or_else(|| Fault::new(ErrorCode::NotFound, "file checkpoint does not exist"))?;
    let record: Record = serde_json::from_slice(&body).map_err(storage_error)?;
    let approval: Approval = serde_json::from_slice(&approval).map_err(storage_error)?;
    runs::visible(db, session, approval.turn).await?;
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
    )
    .await?;
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

/// The approval row, its first file's identity and a later entry's position.
async fn cursor(
    db: &Connection,
    turn: TurnId,
    id: CheckpointId,
) -> Result<Option<(i64, String, Option<i64>)>, Fault> {
    let mut statement = db
        .prepare("SELECT rowid,id FROM file_checkpoints WHERE id=?1 AND turn=?2")
        .await
        .map_err(storage_error)?;
    match statement
        .query_row(params![id.to_string(), turn.to_string()])
        .await
    {
        Ok(row) => {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<i64>(0)?, row.get::<String>(1)?)))(&row);
            let (row, parent) = mapped.map_err(storage_error)?;
            return Ok(Some((row, parent, None)));
        }
        Err(turso::Error::QueryReturnedNoRows) => {}
        Err(error) => return Err(storage_error(error)),
    }
    let mut statement = db
        .prepare(
            "SELECT c.rowid,c.id,e.position FROM file_checkpoint_entries e
             JOIN file_checkpoints c ON c.id=e.checkpoint WHERE e.id=?1 AND c.turn=?2",
        )
        .await
        .map_err(storage_error)?;
    match statement
        .query_row(params![id.to_string(), turn.to_string()])
        .await
    {
        Ok(row) => {
            let mapped: turso::Result<_> = (|row: &turso::Row| {
                Ok((
                    row.get::<i64>(0)?,
                    row.get::<String>(1)?,
                    row.get::<i64>(2)?,
                ))
            })(&row);
            let (row, parent, position) = mapped.map_err(storage_error)?;
            Ok(Some((row, parent, Some(position))))
        }
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(storage_error(error)),
    }
}

/// Later change-set files of one approval row, newest first.
async fn entries(db: &Connection, parent: &str) -> Result<Vec<(i64, CheckpointId)>, Fault> {
    let mut statement = db
        .prepare(
            "SELECT position,id FROM file_checkpoint_entries WHERE checkpoint=?1 ORDER BY position DESC",
        )
        .await
        .map_err(storage_error)?;
    let mut rows = statement.query([parent]).await.map_err(storage_error)?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        let position = row.get::<i64>(0).map_err(storage_error)?;
        let id = row.get::<String>(1).map_err(storage_error)?;
        entries.push((position, id.parse().map_err(storage_error)?));
    }
    Ok(entries)
}
