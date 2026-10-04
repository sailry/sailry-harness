//! Persisted execution context; branch discovery and additional worktrees belong to Git services.
use super::database::storage_error;
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::*;
mod scratch;
pub(super) use scratch::scratch;
pub(super) use scratch::{materialize_session, reserve_session};

pub(super) fn project_root(
    db: &Connection,
    project: ProjectId,
) -> Result<std::path::PathBuf, Fault> {
    let path: Option<String> = db
        .query_row(
            "SELECT path FROM projects WHERE id=?1",
            [project.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    path.map(Into::into)
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "project does not exist on this Node"))
}

pub(super) fn register(db: &Connection, worktree: &Worktree) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,?3,?4)",
        params![
            worktree.id.to_string(),
            worktree.project.map(|id| id.to_string()),
            worktree.path,
            worktree.main
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn finish(
    db: &Connection,
    result: &mut Result<Output, Fault>,
    adopt: bool,
) -> Result<Option<Event>, Fault> {
    if let Ok(Output::WorktreeRemoved { id }) = result {
        let changed = db
            .execute(
                "DELETE FROM worktrees WHERE id=?1 AND main=0",
                [id.to_string()],
            )
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
    if adopt {
        let existing: Option<(String, Option<String>, bool)> = db
            .query_row(
                "SELECT id,project,main FROM worktrees WHERE path=?1",
                [&worktree.path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
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
    register(db, worktree)?;
    Ok(Some(Event::WorktreeRegistered(worktree.clone())))
}

pub(super) fn register_main(db: &Connection, project: &Project) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO worktrees(id,project,path,main) VALUES(?1,?2,?3,1)",
        params![
            WorktreeId::new().to_string(),
            project.id.to_string(),
            project.path
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn select(
    db: &Connection,
    project: ProjectId,
    id: Option<WorktreeId>,
) -> Result<WorktreeId, Fault> {
    let value: Option<String> = match id {
        Some(id) => db.query_row(
            "SELECT id FROM worktrees WHERE id=?1 AND project=?2",
            params![id.to_string(), project.to_string()],
            |row| row.get(0),
        ),
        None => db.query_row(
            "SELECT id FROM worktrees WHERE project=?1 AND main=1",
            [project.to_string()],
            |row| row.get(0),
        ),
    }
    .optional()
    .map_err(storage_error)?;
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

pub(super) fn list(db: &Connection) -> Result<Vec<Worktree>, Fault> {
    let mut query = db
        .prepare("SELECT id,project,path,main FROM worktrees ORDER BY rowid")
        .map_err(storage_error)?;
    query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get(2)?,
                row.get(3)?,
            ))
        })
        .map_err(storage_error)?
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
