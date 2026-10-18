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
        if projects.is_empty() || unique.len() != projects.len() {
            return Err(Fault::invalid("select distinct projects for sharing"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_larger_project_sets() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY); CREATE TABLE removed_projects(project TEXT)").unwrap();
        let projects: Vec<_> = (0..129).map(|_| ProjectId::new()).collect();
        for project in &projects {
            db.execute("INSERT INTO projects VALUES(?1)", [project.to_string()])
                .unwrap();
        }
        validate(&db, Some(&Sharing::Projects(projects.clone()))).unwrap();
        for invalid in [Vec::new(), vec![projects[0], projects[0]]] {
            assert_eq!(
                validate(&db, Some(&Sharing::Projects(invalid)))
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        db.execute(
            "INSERT INTO removed_projects VALUES(?1)",
            [projects[0].to_string()],
        )
        .unwrap();
        assert_eq!(
            validate(&db, Some(&Sharing::Projects(projects)))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
}
