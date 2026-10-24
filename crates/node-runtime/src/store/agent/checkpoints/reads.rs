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
    let boundary = (match before {
        Some(value) => Some(
            (async |id: CheckpointId| {
                {
                    let mut statement = db
                        .prepare("SELECT rowid FROM file_checkpoints WHERE id=?1 AND turn=?2")
                        .await
                        .map_err(storage_error)?;
                    match statement
                        .query_row(params![id.to_string(), turn.to_string()])
                        .await
                    {
                        Ok(row) => {
                            let mapped: turso::Result<_> =
                                (|row: &turso::Row| row.get::<i64>(0))(&row);
                            Some(mapped.map_err(storage_error)?)
                        }
                        Err(turso::Error::QueryReturnedNoRows) => None,
                        Err(error) => return Err(storage_error(error).into()),
                    }
                }
                .ok_or_else(|| {
                    Fault::new(
                        ErrorCode::WrongTarget,
                        "file checkpoint cursor belongs to another turn",
                    )
                })
            })(value)
            .await,
        ),
        None => None,
    })
    .transpose()?;
    let mut query = db.prepare("SELECT id FROM file_checkpoints WHERE turn=?1 AND rowid<?2 ORDER BY rowid DESC LIMIT ?3").await
        .map_err(storage_error)?;
    let ids = {
        let mut rows = query
            .query(params![
                turn.to_string(),
                boundary.unwrap_or(i64::MAX),
                i64::from(limit) + 1
            ])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
    .collect::<Result<Vec<CheckpointId>, Fault>>()?;
    let more = ids.len() > usize::from(limit);
    let files = {
        let mut selected = Vec::new();
        for value in ids.into_iter().take(usize::from(limit)) {
            selected.push((async |id: CheckpointId| file(db, session, id).await)(value).await?);
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

pub(super) async fn content(
    db: &Connection,
    session: SessionId,
    id: CheckpointId,
) -> Result<checkpoint::Content, Fault> {
    let file = file(db, session, id).await?;
    let (before, after): (Option<String>, String) = {
        let mut statement = db
            .prepare("SELECT before_text,after_text FROM file_checkpoints WHERE id=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
        mapped.map_err(storage_error)?
    };
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
    let row = {
        let mut statement = db.prepare("SELECT c.body,a.body,t.caller,a.request FROM file_checkpoints c JOIN agent_approvals a ON a.id=c.approval
         JOIN turns t ON t.id=c.turn WHERE c.id=?1").await.map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| {
                    Ok((
                        row.get::<Vec<u8>>(0)?,
                        row.get::<Vec<u8>>(1)?,
                        row.get::<Vec<u8>>(2)?,
                        row.get::<String>(3)?,
                    ))
                })(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
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
