//! Persisted execution context; branch discovery and additional worktrees belong to Git services.
use super::database::storage_error;
use sailry_protocol::*;
use turso::{Connection, params};
mod scratch;
pub(super) use scratch::scratch;
pub(super) use scratch::{materialize_session, reserve_session};

pub(super) async fn project_root(
    db: &Connection,
    project: ProjectId,
) -> Result<std::path::PathBuf, Fault> {
    let path: Option<String> = {
        let mut statement = db
            .prepare("SELECT path FROM projects WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([project.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    path.map(Into::into)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "project does not exist on this Node"))
}

/// Rejects a worktree whose checkout was removed while its sessions kept history.
pub(super) async fn present(db: &Connection, id: WorktreeId) -> Result<(), Fault> {
    let removed: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM removed_worktrees WHERE worktree=?1)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    if removed {
        Err(Fault::new(ErrorCode::NotFound, "worktree was removed"))
    } else {
        Ok(())
    }
}

pub(super) async fn register(db: &Connection, worktree: &Worktree) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,?3,?4)",
        params![
            worktree.id.to_string(),
            worktree.project.map(|id| id.to_string()),
            worktree.path.as_str(),
            worktree.main
        ],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn finish(
    db: &Connection,
    result: &mut Result<Output, Fault>,
    adopt: bool,
) -> Result<Option<Event>, Fault> {
    if let Ok(Output::WorktreeRemoved { id }) = result {
        let id_text = id.to_string();
        let referenced: bool = {
            let mut statement = db
                .prepare(
                    "SELECT EXISTS(SELECT 1 FROM sessions WHERE worktree=?1)
                 OR EXISTS(SELECT 1 FROM session_revisions WHERE worktree=?1)
                 OR EXISTS(SELECT 1 FROM agent_approval_rules WHERE worktree=?1)",
                )
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row([id_text.as_str()])
                .await
                .map_err(storage_error)?;
            let mapped: turso::Result<_> = row.get(0);
            mapped.map_err(storage_error)?
        };
        let changed = if referenced {
            // Session history keeps its foreign keys; the tombstone hides the checkout.
            for table in ["terminals", "code_startups", "hook_settings"] {
                db.execute(
                    &format!("DELETE FROM {table} WHERE worktree=?1"),
                    [id_text.as_str()],
                )
                .await
                .map_err(storage_error)?;
            }
            db.execute(
                "INSERT INTO removed_worktrees(worktree) SELECT id FROM worktrees
                 WHERE id=?1 AND main=0 AND id NOT IN (SELECT worktree FROM removed_worktrees)",
                [id_text.as_str()],
            )
            .await
        } else {
            db.execute(
                "DELETE FROM worktrees WHERE id=?1 AND main=0",
                [id_text.as_str()],
            )
            .await
        }
        .map_err(storage_error)?;
        if changed != 1 {
            return Err(storage_error(
                "worktree registration changed during removal",
            ));
        }
        return Ok(Some(Event::WorktreeRemoved { id: *id }));
    }
    let Ok(Output::Worktree(worktree)) = result else {
        return Ok(None);
    };
    let removed: Option<(String, Option<String>)> = {
        let mut statement = db.prepare("SELECT w.id,w.project FROM worktrees w JOIN removed_worktrees r ON r.worktree=w.id WHERE w.path=?1").await.map_err(storage_error)?;
        match statement.query_row([worktree.path.as_str()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    if let Some((id, project)) = removed {
        if project != worktree.project.map(|id| id.to_string()) {
            *result = Err(Fault::new(
                ErrorCode::Conflict,
                "worktree is registered to another project",
            ));
            return Ok(None);
        }
        // A checkout restored at a removed path resumes its retained sessions.
        db.execute(
            "DELETE FROM removed_worktrees WHERE worktree=?1",
            [id.as_str()],
        )
        .await
        .map_err(storage_error)?;
        worktree.id = id.parse().map_err(storage_error)?;
        worktree.main = false;
        return Ok(Some(Event::WorktreeRegistered(worktree.clone())));
    }
    if adopt {
        let existing: Option<(String, Option<String>, bool)> = {
            let mut statement = db
                .prepare("SELECT id,project,main FROM worktrees WHERE path=?1")
                .await
                .map_err(storage_error)?;
            match statement.query_row([worktree.path.as_str()]).await {
                Ok(row) => {
                    let mapped: turso::Result<_> =
                        (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))(&row);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        if let Some((id, project, main)) = existing {
            if project != worktree.project.map(|id| id.to_string()) {
                *result = Err(Fault::new(
                    ErrorCode::Conflict,
                    "worktree is registered to another project",
                ));
                return Ok(None);
            }
            worktree.id = id.parse().map_err(storage_error)?;
            worktree.main = main;
            return Ok(None);
        }
    }
    register(db, worktree).await?;
    Ok(Some(Event::WorktreeRegistered(worktree.clone())))
}

pub(super) async fn register_main(db: &Connection, project: &Project) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,?3,1)",
        params![
            WorktreeId::new().to_string(),
            project.id.to_string(),
            project.path.as_str()
        ],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn select(
    db: &Connection,
    project: ProjectId,
    id: Option<WorktreeId>,
) -> Result<WorktreeId, Fault> {
    let result: turso::Result<String> = match id {
        Some(id) => (async { let mut statement = db.prepare("SELECT id FROM worktrees WHERE id=?1 AND project=?2 AND id NOT IN (SELECT worktree FROM removed_worktrees)").await?;
let row = statement.query_row(params![id.to_string(), project.to_string()]).await?;
let mapped: turso::Result<_> = row.get(0);
mapped }).await,
        None => (async { let mut statement = db.prepare("SELECT id FROM worktrees WHERE project=?1 AND main=1").await?;
let row = statement.query_row([project.to_string()]).await?;
let mapped: turso::Result<_> = row.get(0);
mapped }).await,
    };
    let value = match result {
        Ok(value) => Some(value),
        Err(turso::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(storage_error(error)),
    };
    value
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::NotFound,
                "worktree does not belong to the selected project",
            )
        })?
        .parse()
        .map_err(storage_error)
}

pub(super) async fn list(db: &Connection) -> Result<Vec<Worktree>, Fault> {
    let mut query = db
        .prepare("SELECT id,project,path,main FROM worktrees WHERE id NOT IN (SELECT worktree FROM removed_worktrees) ORDER BY rowid").await
        .map_err(storage_error)?;
    {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| {
                Ok((
                    row.get::<String>(0)?,
                    row.get::<Option<String>>(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| {
        let (id, project, path, main) = row.map_err(storage_error)?;
        Ok(Worktree {
            id: id.parse().map_err(storage_error)?,
            project: project
                .map(|id| id.parse())
                .transpose()
                .map_err(storage_error)?,
            path,
            main,
        })
    })
    .collect()
}
