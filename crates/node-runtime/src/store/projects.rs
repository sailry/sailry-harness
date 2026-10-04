//! Project creation behavior: Sailry Code d9b56405, create_project_dialog.dart.
use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{projects::*, *};
use std::path::{Path, PathBuf};

pub(super) fn prepare(draft: &Draft) -> Result<PathBuf, Fault> {
    validate(&draft.name, &draft.path, &draft.appearance)?;
    let path = Path::new(&draft.path);
    match &draft.source {
        Source::Local => directory(path),
        Source::Clone { url, branch } => {
            if url.trim().is_empty()
                || url.len() > 4096
                || url.contains(['\n', '\r', '\0'])
                || branch
                    .as_ref()
                    .is_some_and(|branch| branch.is_empty() || branch.len() > 256)
            {
                return Err(invalid("repository URL or branch is invalid"));
            }
            if let Ok(parsed) = url::Url::parse(url) {
                if !Path::new(url).is_absolute()
                    && (!matches!(parsed.scheme(), "http" | "https" | "ssh" | "file")
                        || parsed.password().is_some()
                        || (matches!(parsed.scheme(), "http" | "https")
                            && !parsed.username().is_empty()))
                {
                    return Err(invalid("use a repository URL without embedded credentials"));
                }
            } else if !Path::new(url).is_absolute() && !(url.contains('@') && url.contains(':')) {
                return Err(invalid("repository URL is invalid"));
            }
            let parent = path
                .parent()
                .ok_or_else(|| invalid("clone destination requires a parent directory"))?;
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid("clone destination requires a name"))?;
            crate::files::path::entry_components(name)?;
            let path = directory(parent)?.join(name);
            if path.symlink_metadata().is_ok() {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "clone destination already exists",
                ));
            }
            Ok(path)
        }
    }
}

pub(super) fn execute(draft: &Draft, path: &Path) -> Result<Output, Fault> {
    if let Source::Clone { url, branch } = &draft.source {
        std::fs::create_dir(path).map_err(crate::files::io_error)?;
        let mut fetch = git2::FetchOptions::new();
        fetch.remote_callbacks(crate::git::credentials::callbacks());
        let mut builder = git2::build::RepoBuilder::new();
        builder.fetch_options(fetch);
        if let Some(branch) = branch {
            builder.branch(branch);
        }
        builder.clone(url, path).map_err(|_| {
            Fault::new(
                ErrorCode::OutcomeUnknown,
                "clone did not finish; inspect the retained destination directory",
            )
        })?;
    }
    Ok(Output::Project(Project {
        id: ProjectId::new(),
        name: draft.name.trim().into(),
        path: text(path)?,
        appearance: draft.appearance.clone(),
    }))
}

pub(super) fn register(
    db: &Connection,
    name: &str,
    path: &str,
    appearance: Appearance,
) -> Result<(Output, Option<Event>), Fault> {
    validate(name, path, &appearance)?;
    let mut project = Project {
        id: ProjectId::new(),
        name: name.trim().into(),
        path: text(&directory(Path::new(path))?)?,
        appearance,
    };
    restore(db, &mut project)?;
    insert(db, &project)?;
    Ok((
        Output::Project(project.clone()),
        Some(Event::ProjectRegistered(project)),
    ))
}

pub(super) fn finish(
    db: &Connection,
    command: &Command,
    result: &mut sailry_link::Response,
) -> Result<Option<Event>, Fault> {
    if !matches!(command, Command::CreateProject(_)) {
        return Ok(None);
    }
    let Ok(Output::Project(project)) = result else {
        return Ok(None);
    };
    restore(db, project)?;
    match insert(db, project) {
        Ok(()) => Ok(Some(Event::ProjectRegistered(project.clone()))),
        // Duplicate detection precedes every write in insert.
        Err(error) if error.code == ErrorCode::Conflict => {
            *result = Err(error);
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn insert(db: &Connection, project: &Project) -> Result<(), Fault> {
    if get(db, project.id)?.as_ref() == Some(project) {
        return Ok(());
    }
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE path=?1)",
            [&project.path],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if exists {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "project directory is already registered",
        ));
    }
    db.execute(
        "INSERT INTO projects(id,name,path,appearance) VALUES(?1,?2,?3,?4)",
        params![
            project.id.to_string(),
            project.name,
            project.path,
            encode(&project.appearance)?
        ],
    )
    .map_err(storage_error)?;
    super::worktrees::register_main(db, project)
}

pub(super) fn list(db: &Connection) -> Result<Vec<Project>, Fault> {
    let mut query = db.prepare("SELECT id,name,path,appearance FROM projects WHERE id NOT IN (SELECT project FROM removed_projects) ORDER BY rowid").map_err(storage_error)?;
    query
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (id, name, path, appearance) = row.map_err(storage_error)?;
            Ok(Project {
                id: id.parse().map_err(storage_error)?,
                name,
                path,
                appearance: serde_json::from_slice(&appearance).map_err(storage_error)?,
            })
        })
        .collect()
}

fn get(db: &Connection, id: ProjectId) -> Result<Option<Project>, Fault> {
    let row: Option<(String, String, Vec<u8>)> = db
        .query_row(
            "SELECT name,path,appearance FROM projects WHERE id=?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    row.map(|(name, path, appearance)| {
        Ok(Project {
            id,
            name,
            path,
            appearance: serde_json::from_slice(&appearance).map_err(storage_error)?,
        })
    })
    .transpose()
}

fn check(db: &Connection, expected: &Project) -> Result<(), Fault> {
    if get(db, expected.id)?.as_ref() != Some(expected) {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "project changed; reload before saving",
        ));
    }
    let removed: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM removed_projects WHERE project=?1)",
            [expected.id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if removed {
        return Err(Fault::new(ErrorCode::NotFound, "project was removed"));
    }
    Ok(())
}

fn restore(db: &Connection, project: &mut Project) -> Result<(), Fault> {
    let id: Option<String> = db
        .query_row(
            "SELECT p.id FROM projects p JOIN removed_projects r ON r.project=p.id WHERE p.path=?1",
            [&project.path],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(id) = id {
        project.id = id.parse().map_err(storage_error)?;
        db.execute("DELETE FROM removed_projects WHERE project=?1", [&id])
            .map_err(storage_error)?;
        db.execute(
            "UPDATE projects SET name=?2,appearance=?3 WHERE id=?1",
            params![id, project.name, encode(&project.appearance)?],
        )
        .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) fn update(
    db: &Connection,
    expected: &Project,
    name: &str,
    path: &str,
    appearance: &Appearance,
) -> Result<(Output, Option<Event>), Fault> {
    check(db, expected)?;
    validate(name, path, appearance)?;
    let path = if path == expected.path {
        path.to_owned()
    } else {
        text(&directory(Path::new(path))?)?
    };
    if path != expected.path {
        let used: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE project=?1) OR EXISTS(SELECT 1 FROM worktrees WHERE project=?1 AND main=0) OR EXISTS(SELECT 1 FROM terminals WHERE worktree IN (SELECT id FROM worktrees WHERE project=?1))",
            [expected.id.to_string()], |row| row.get(0)).map_err(storage_error)?;
        if used {
            return Err(Fault::new(
                ErrorCode::Busy,
                "project path is bound to existing resources",
            ));
        }
        let duplicate: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM worktrees WHERE path=?1)",
                [&path],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if duplicate {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "directory is already registered",
            ));
        }
        db.execute(
            "UPDATE worktrees SET path=?2 WHERE project=?1 AND main=1",
            params![expected.id.to_string(), path],
        )
        .map_err(storage_error)?;
    }
    let project = Project {
        id: expected.id,
        name: name.trim().into(),
        path,
        appearance: appearance.clone(),
    };
    db.execute(
        "UPDATE projects SET name=?2,path=?3,appearance=?4 WHERE id=?1",
        params![
            project.id.to_string(),
            project.name,
            project.path,
            encode(&project.appearance)?
        ],
    )
    .map_err(storage_error)?;
    Ok((
        Output::Project(project.clone()),
        Some(Event::ProjectChanged(project)),
    ))
}

pub(super) fn remove(
    db: &Connection,
    expected: &Project,
) -> Result<(Output, Option<Event>), Fault> {
    check(db, expected)?;
    let active: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN sessions s ON s.id=t.session WHERE s.project=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))",
        [expected.id.to_string()], |row| row.get(0)).map_err(storage_error)?;
    if active {
        return Err(Fault::new(ErrorCode::Busy, "project has pending tasks"));
    }
    let trees = super::worktrees::list(db)?;
    if super::terminals::list(db)?.iter().any(|terminal| {
        terminal.status == terminal::Status::Running
            && trees
                .iter()
                .any(|tree| tree.project == Some(expected.id) && Some(tree.id) == terminal.worktree)
    }) {
        return Err(Fault::new(ErrorCode::Busy, "project has running terminals"));
    }
    db.execute(
        "INSERT INTO removed_projects(project) VALUES(?1)",
        [expected.id.to_string()],
    )
    .map_err(storage_error)?;
    Ok((
        Output::ProjectRemoved { id: expected.id },
        Some(Event::ProjectRemoved { id: expected.id }),
    ))
}

fn validate(name: &str, path: &str, appearance: &Appearance) -> Result<(), Fault> {
    if name.trim().is_empty()
        || name.len() > 128
        || path.len() > 4096
        || !Path::new(path).is_absolute()
    {
        return Err(invalid(
            "an absolute directory and a nonempty project name are required",
        ));
    }
    if !ICONS.contains(&appearance.icon.as_str()) || !COLORS.contains(&appearance.color.as_str()) {
        return Err(invalid("project appearance is invalid"));
    }
    Ok(())
}

fn directory(path: &Path) -> Result<PathBuf, Fault> {
    let path = path
        .canonicalize()
        .map_err(|_| invalid("project directory is not accessible on this Node"))?;
    if !path.is_dir() {
        return Err(invalid("project path is not a directory"));
    }
    Ok(path)
}

fn text(path: &Path) -> Result<String, Fault> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("project path is not UTF-8"))
}
fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
