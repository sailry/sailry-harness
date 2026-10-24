//! Sharing is evaluated by the execution Node, independently of desktop pickers.
use super::{commands, database::storage_error};
use sailry_protocol::{
    connection::{Resource, Sharing},
    *,
};
use turso::Connection;

pub(super) async fn bind(
    db: &Connection,
    profile: Option<&std::path::Path>,
    id: SessionId,
    revision: u64,
    target: Resource,
) -> Result<(Output, Option<Event>), Fault> {
    let mut session = commands::session(db, id).await?;
    commands::check_revision(session.revision, revision)?;
    super::sessions::writable(&session)?;
    let used: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM turns WHERE session=?1)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
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
    session.worktree = workspace(db, profile, target).await?;
    session.project = None;
    session.config.resource = Some(target);
    db.execute(
        "UPDATE sessions SET project=NULL WHERE id=?1",
        [id.to_string()],
    )
    .await
    .map_err(storage_error)?;
    commands::revise_session(db, session).await
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

pub(super) async fn validate(db: &Connection, sharing: Option<&Sharing>) -> Result<(), Fault> {
    if let Some(Sharing::Projects(projects)) = sharing {
        let unique: std::collections::BTreeSet<_> = projects.iter().collect();
        if projects.is_empty() || unique.len() != projects.len() {
            return Err(Fault::invalid("select distinct projects for sharing"));
        }
        for project in projects {
            let exists: bool = {
                let mut statement = db.prepare("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND id NOT IN (SELECT project FROM removed_projects))").await.map_err(storage_error)?;
                let row = statement
                    .query_row([project.to_string()])
                    .await
                    .map_err(storage_error)?;
                let mapped: turso::Result<_> = row.get(0);
                mapped.map_err(storage_error)?
            };
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

    #[tokio::test]
    async fn accepts_larger_project_sets() {
        let engine = crate::store::database::engine(std::path::Path::new(":memory:"))
            .await
            .unwrap();
        let db = crate::store::database::connect(&engine).await.unwrap();
        db.execute_batch("CREATE TABLE projects(id TEXT PRIMARY KEY); CREATE TABLE removed_projects(project TEXT)").await.unwrap();
        let projects: Vec<_> = (0..129).map(|_| ProjectId::new()).collect();
        for project in &projects {
            db.execute("INSERT INTO projects VALUES(?1)", [project.to_string()])
                .await
                .unwrap();
        }
        validate(&db, Some(&Sharing::Projects(projects.clone())))
            .await
            .unwrap();
        for invalid in [Vec::new(), vec![projects[0], projects[0]]] {
            assert_eq!(
                validate(&db, Some(&Sharing::Projects(invalid)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        db.execute(
            "INSERT INTO removed_projects VALUES(?1)",
            [projects[0].to_string()],
        )
        .await
        .unwrap();
        assert_eq!(
            validate(&db, Some(&Sharing::Projects(projects)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
}
