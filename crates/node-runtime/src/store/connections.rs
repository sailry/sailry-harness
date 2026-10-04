//! Sharing is evaluated by the execution Node, independently of desktop pickers.
use super::{commands, database::storage_error};
use rusqlite::Connection;
use sailry_protocol::{
    connection::{Resource, Sharing},
    *,
};

pub(super) fn bind(
    db: &Connection,
    profile: Option<&std::path::Path>,
    id: SessionId,
    revision: u64,
    target: Resource,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = commands::session(db, id)?;
    commands::check_revision(session.revision, revision)?;
    super::sessions::writable(&session)?;
    let used: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM turns WHERE session=?1)",
            [id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if session.config.resource.is_some()
        || session.config.assistant.is_some()
        || used
        || session.archived
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "only a new conversation can bind a connection",
        ));
    }
    session.worktree = workspace(db, profile, target)?;
    session.project = None;
    session.config.resource = Some(target);
    db.execute(
        "UPDATE sessions SET project=NULL WHERE id=?1",
        [id.to_string()],
    )
    .map_err(storage_error)?;
    commands::revise_session(db, session)
}

pub(super) fn permits(
    bound: Option<Resource>,
    project: Option<ProjectId>,
    target: Resource,
    sharing: Option<&Sharing>,
) -> bool {
    match bound {
        Some(bound) => bound == target,
        None => {
            project.is_some_and(|project| sharing.is_some_and(|sharing| sharing.includes(project)))
        }
    }
}

pub(super) fn validate(db: &Connection, sharing: Option<&Sharing>) -> Result<(), Fault> {
    if let Some(Sharing::Projects(projects)) = sharing {
        let unique: std::collections::BTreeSet<_> = projects.iter().collect();
        if projects.is_empty() || projects.len() > 128 || unique.len() != projects.len() {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "select distinct projects for sharing",
            ));
        }
        for project in projects {
            let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND id NOT IN (SELECT project FROM removed_projects))", [project.to_string()], |row| row.get(0)).map_err(storage_error)?;
            if !exists {
                return Err(Fault::new(
                    ErrorCode::NotFound,
                    "shared project is unavailable",
                ));
            }
        }
    }
    Ok(())
}
fn missing() -> Fault {
    Fault::new(ErrorCode::NotFound, "connection is unavailable")
}

mod workspace;
pub(super) use workspace::workspace;
