use super::database::{encode, storage_error};
use sailry_protocol::{
    AttachmentId, Command, ErrorCode, Fault, NodeId, Output, WorktreeId, attachment::Attachment,
};
use std::{collections::BTreeSet, path::Path};
use turso::{Connection, params};

pub(super) mod turns;

pub(super) async fn read(
    db: &Connection,
    worktree: WorktreeId,
    id: AttachmentId,
) -> Result<Attachment, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM attachments WHERE id=?1 AND worktree=?2")
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![id.to_string(), worktree.to_string()])
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
    let attachment: Attachment =
        serde_json::from_slice(&body.ok_or_else(missing)?).map_err(storage_error)?;
    if attachment.id != id || attachment.spec.worktree != worktree {
        return Err(storage_error("attachment owner mismatch"));
    }
    Ok(attachment)
}

pub(super) async fn discard(
    db: &Connection,
    caller: NodeId,
    worktree: WorktreeId,
    id: AttachmentId,
) -> Result<(), Fault> {
    let owner: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT caller FROM attachments WHERE id=?1 AND worktree=?2")
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![id.to_string(), worktree.to_string()])
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
    if owner.as_deref() != Some(&caller.0[..]) {
        return Err(missing());
    }
    let used: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM turn_attachments WHERE attachment=?1)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if used {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "attachment is used by a conversation",
        ));
    }
    db.execute("DELETE FROM attachments WHERE id=?1", [id.to_string()])
        .await
        .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn finish(
    db: &Connection,
    caller: NodeId,
    command: &Command,
    result: &Result<Output, Fault>,
) -> Result<(), Fault> {
    if let (Command::FinishAttachmentUpload { worktree, .. }, Ok(Output::Attachment(attachment))) =
        (command, result)
    {
        if attachment.spec.worktree != *worktree {
            return Err(storage_error("attachment owner mismatch"));
        }
        db.execute(
            "INSERT INTO attachments(id,worktree,caller,body) VALUES(?1,?2,?3,?4)",
            params![
                attachment.id.to_string(),
                worktree.to_string(),
                &caller.0[..],
                encode(attachment)?
            ],
        )
        .await
        .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) async fn collect(db: &Connection, profile: Option<&Path>) {
    let Some(profile) = profile else { return };
    let result = (async || {
        let mut query = db
            .prepare("SELECT id FROM attachments")
            .await
            .map_err(storage_error)?;
        let retained: BTreeSet<AttachmentId> = {
            let mut rows = query.query(()).await.map_err(storage_error)?;
            let mut values = Vec::new();
            while let Some(row) = rows.next().await.map_err(storage_error)? {
                let mapped: turso::Result<_> = row.get::<String>(0);
                values.push(mapped);
            }
            values.into_iter()
        }
        .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
        .collect::<Result<_, _>>()?;
        crate::files::attachments::collect(profile, &retained)
    })()
    .await;
    if let Err(error) = result {
        eprintln!("attachment cleanup failed: {error}");
    }
}

fn missing() -> Fault {
    Fault::new(ErrorCode::NotFound, "attachment is unavailable")
}
