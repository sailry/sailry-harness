use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    AttachmentId, Command, ErrorCode, Fault, NodeId, Output, WorktreeId, attachment::Attachment,
};
use std::{collections::BTreeSet, path::Path};

pub(super) mod turns;

pub(super) fn read(
    db: &Connection,
    worktree: WorktreeId,
    id: AttachmentId,
) -> Result<Attachment, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM attachments WHERE id=?1 AND worktree=?2",
            params![id.to_string(), worktree.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let attachment: Attachment =
        serde_json::from_slice(&body.ok_or_else(missing)?).map_err(storage_error)?;
    if attachment.id != id || attachment.spec.worktree != worktree {
        return Err(storage_error("attachment owner mismatch"));
    }
    Ok(attachment)
}

pub(super) fn discard(
    db: &Connection,
    caller: NodeId,
    worktree: WorktreeId,
    id: AttachmentId,
) -> Result<(), Fault> {
    let owner: Option<Vec<u8>> = db
        .query_row(
            "SELECT caller FROM attachments WHERE id=?1 AND worktree=?2",
            params![id.to_string(), worktree.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if owner.as_deref() != Some(&caller.0[..]) {
        return Err(missing());
    }
    let used: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM turn_attachments WHERE attachment=?1)",
            [id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if used {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "attachment is used by a conversation",
        ));
    }
    db.execute("DELETE FROM attachments WHERE id=?1", [id.to_string()])
        .map_err(storage_error)?;
    Ok(())
}

pub(super) fn finish(
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
        .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) fn collect(db: &Connection, profile: Option<&Path>) {
    let Some(profile) = profile else { return };
    let result = (|| {
        let mut query = db
            .prepare("SELECT id FROM attachments")
            .map_err(storage_error)?;
        let retained: BTreeSet<AttachmentId> = query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?
            .map(|row| row.map_err(storage_error)?.parse().map_err(storage_error))
            .collect::<Result<_, _>>()?;
        crate::files::attachments::collect(profile, &retained)
    })();
    if let Err(error) = result {
        eprintln!("attachment cleanup failed: {error}");
    }
}

fn missing() -> Fault {
    Fault::new(ErrorCode::NotFound, "attachment is unavailable")
}
