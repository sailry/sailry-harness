//! Headless assistants use Node-owned directories with ordinary worktree identity.
use super::*;
use std::path::Path;

/// Reserve an execution identity without creating files for a chat-only session.
pub(in crate::store) async fn reserve_session(
    db: &Connection,
    profile: Option<&Path>,
    session: SessionId,
) -> Result<WorktreeId, Fault> {
    let profile = profile.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "session workspace storage is unavailable",
        )
    })?;
    let root = crate::workspaces::session(profile, session);
    let path = root
        .to_str()
        .ok_or_else(|| storage_error("session workspace path is not UTF-8"))?;
    let id = WorktreeId::new();
    register(
        db,
        &Worktree {
            id,
            project: None,
            path: path.into(),
            main: false,
        },
    )
    .await?;
    Ok(id)
}

/// Only Node-reserved session directories are created on first resource access.
/// Missing user project directories must continue to report an error.
pub(in crate::store) async fn materialize_session(
    db: &Connection,
    profile: Option<&Path>,
    worktree: WorktreeId,
    root: &Path,
) -> Result<(), Fault> {
    let Some(profile) = profile else {
        return Ok(());
    };
    let Some(name) = root.file_name().and_then(|name| name.to_str()) else {
        return Ok(());
    };
    let Ok(session) = name.parse::<SessionId>() else {
        return Ok(());
    };
    if root != crate::workspaces::session(profile, session) {
        return Ok(());
    }
    let owned: bool = {
        let mut statement = db.prepare("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND worktree=?2 AND project IS NULL)").await.map_err(storage_error)?;
        let row = statement
            .query_row(params![session.to_string(), worktree.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
    if owned {
        std::fs::create_dir_all(root).map_err(storage_error)?;
    }
    Ok(())
}

pub(in crate::store) async fn scratch(db: &Connection, root: &Path) -> Result<WorktreeId, Fault> {
    std::fs::create_dir_all(root).map_err(storage_error)?;
    let path = root
        .to_str()
        .ok_or_else(|| storage_error("assistant workspace path is not UTF-8"))?;
    let existing: Option<String> = {
        let mut statement = db
            .prepare("SELECT id FROM worktrees WHERE project IS NULL AND path=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([path]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    if let Some(id) = existing {
        return id.parse().map_err(storage_error);
    }
    let id = WorktreeId::new();
    register(
        db,
        &Worktree {
            id,
            project: None,
            path: path.into(),
            main: false,
        },
    )
    .await?;
    Ok(id)
}
