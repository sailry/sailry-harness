//! Conversation connection edits contain public configuration and preserve stored credentials.
use super::*;
use serde::Deserialize;

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DatabaseInput {
    pub connection: Option<DatabaseId>,
    pub expected_revision: Option<u64>,
    pub name: Option<String>,
    pub engine: Option<database::Engine>,
    pub path: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub database: Option<String>,
    pub read_only: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SshInput {
    pub connection: Option<SshId>,
    pub expected_revision: Option<u64>,
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub authentication: Option<ssh::Authentication>,
    pub credential: Option<ssh::Credential>,
    pub key_path: Option<String>,
}

fn required<T>(value: Option<T>, name: &str) -> Result<T, Fault> {
    value.ok_or_else(|| Fault::invalid(format!("connection requires {name}")))
}

fn revision<T>(id: Option<T>, revision: Option<u64>) -> Result<u64, Fault> {
    match (id.is_some(), revision) {
        (false, None) => Ok(0),
        (true, Some(revision)) if revision > 0 => Ok(revision),
        _ => Err(Fault::invalid(
            "connection update requires its exact revision",
        )),
    }
}

pub(super) async fn database(
    db: &Connection,
    turn: TurnId,
    package: &plugin::Reference,
    captured: &[Resource],
    input: DatabaseInput,
) -> Result<Command, Fault> {
    let expected_revision = revision(input.connection, input.expected_revision)?;
    let previous = (match input.connection {
        Some(value) => Some(
            (async |id| {
                check(db, turn, package, Resource::Database(id), captured).await?;
                super::super::super::databases::read(db, id)
                    .await?
                    .ok_or_else(|| {
                        Fault::new(ErrorCode::NotFound, "database connection does not exist")
                    })
            })(value)
            .await,
        ),
        None => None,
    })
    .transpose()?;
    if previous.is_none() {
        creation(db, turn).await?;
    }
    let mut profile = previous.clone().unwrap_or(database::Profile {
        id: DatabaseId::new(),
        revision: 0,
        sharing: sharing(db, turn).await?,
        name: String::new(),
        connection: database::Connection::Sqlite {
            path: String::new(),
        },
        read_only: true,
    });
    if let Some(name) = input.name {
        profile.name = name;
    }
    if let Some(read_only) = input.read_only {
        profile.read_only = read_only;
    }
    let connection_edit = input.engine.is_some()
        || input.path.is_some()
        || input.host.is_some()
        || input.port.is_some()
        || input.username.is_some()
        || input.database.is_some();
    if previous.is_none() || connection_edit {
        let engine = required(
            input
                .engine
                .or_else(|| previous.as_ref().map(|p| p.connection.engine())),
            "engine",
        )?;
        profile.connection = match engine {
            database::Engine::Sqlite => {
                if input.host.is_some()
                    || input.port.is_some()
                    || input.username.is_some()
                    || input.database.is_some()
                {
                    return Err(Fault::invalid("SQLite connection accepts only a path"));
                }
                let existing = match &profile.connection {
                    database::Connection::Sqlite { path } if previous.is_some() => {
                        Some(path.clone())
                    }
                    _ => None,
                };
                database::Connection::Sqlite {
                    path: match input.path {
                        Some(path) => sqlite_path(db, turn, path).await?,
                        None => required(existing, "path")?,
                    },
                }
            }
            engine => {
                if input.path.is_some() {
                    return Err(Fault::invalid(
                        "network database connection does not accept a path",
                    ));
                }
                if input.engine.is_none()
                    && matches!(
                        profile.connection,
                        database::Connection::Socket { .. } | database::Connection::Ssh { .. }
                    )
                {
                    return Err(Fault::invalid(
                        "changing a socket or tunnel requires an explicit engine",
                    ));
                }
                let existing = match &profile.connection {
                    database::Connection::Network {
                        host,
                        port,
                        username,
                        database,
                        tls,
                        ..
                    } => Some((
                        host.clone(),
                        *port,
                        username.clone(),
                        database.clone(),
                        *tls,
                    )),
                    _ => None,
                };
                database::Connection::Network {
                    engine,
                    host: required(
                        input
                            .host
                            .or_else(|| existing.as_ref().map(|p| p.0.clone())),
                        "host",
                    )?,
                    port: required(
                        input.port.or_else(|| existing.as_ref().map(|p| p.1)),
                        "port",
                    )?,
                    username: required(
                        input
                            .username
                            .or_else(|| existing.as_ref().map(|p| p.2.clone())),
                        "username",
                    )?,
                    database: required(
                        input
                            .database
                            .or_else(|| existing.as_ref().map(|p| p.3.clone())),
                        "database",
                    )?,
                    tls: existing.map_or(database::Tls::Prefer, |p| p.4),
                }
            }
        };
    }
    Ok(Command::SaveDatabase {
        profile,
        expected_revision,
        password: None,
    })
}

async fn sqlite_path(db: &Connection, turn: TurnId, path: String) -> Result<String, Fault> {
    if std::path::Path::new(&path).is_absolute() {
        return Ok(path);
    }
    let parts = crate::files::path::components(path.strip_prefix("./").unwrap_or(&path), false)?;
    let run = super::super::calls::active(db, turn).await?;
    let root: Option<String> = {
        let mut statement = db
            .prepare("SELECT path FROM worktrees WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([run.worktree.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    let root = root.ok_or_else(|| Fault::new(ErrorCode::NotFound, "worktree does not exist"))?;
    let mut resolved = std::path::PathBuf::from(root);
    if !resolved.is_absolute() {
        return Err(Fault::invalid("worktree path must be absolute"));
    }
    for part in parts {
        resolved.push(part);
    }
    resolved
        .into_os_string()
        .into_string()
        .map_err(|_| Fault::invalid("database path must be UTF-8"))
}

pub(super) async fn ssh(
    db: &Connection,
    turn: TurnId,
    package: &plugin::Reference,
    captured: &[Resource],
    input: SshInput,
) -> Result<Command, Fault> {
    let expected_revision = revision(input.connection, input.expected_revision)?;
    let previous = (match input.connection {
        Some(value) => Some(
            (async |id| {
                check(db, turn, package, Resource::Ssh(id), captured).await?;
                super::super::super::ssh::read(db, id)
                    .await?
                    .ok_or_else(|| Fault::new(ErrorCode::NotFound, "SSH connection does not exist"))
            })(value)
            .await,
        ),
        None => None,
    })
    .transpose()?;
    if previous.is_none() {
        creation(db, turn).await?;
    }
    let authentication = required(
        input
            .authentication
            .or_else(|| previous.as_ref().map(|p| p.authentication)),
        "authentication",
    )?;
    if input.credential.is_some() && input.key_path.is_some() {
        return Err(Fault::invalid(
            "SSH connection accepts a credential or a key path, not both",
        ));
    }
    let credential = if let Some(credential) = input.credential {
        if credential.authentication() != authentication {
            return Err(Fault::invalid(
                "SSH credential authentication does not match",
            ));
        }
        if let ssh::Credential::KeyPath { path, .. } = &credential {
            key_path(path.expose())?;
        }
        Some(credential)
    } else if let Some(path) = input.key_path {
        key_path(&path)?;
        if authentication != ssh::Authentication::KeyPath {
            return Err(Fault::invalid(
                "SSH key path requires an absolute path and key_path authentication",
            ));
        }
        if previous
            .as_ref()
            .is_some_and(|p| p.authentication == ssh::Authentication::KeyPath)
        {
            // A path edit must retain the existing optional passphrase inside the Node.
            let body: String = {
                let mut statement = db
                    .prepare("SELECT credential FROM ssh_profiles WHERE id=?1")
                    .await
                    .map_err(storage_error)?;
                let row = statement
                    .query_row([previous.as_ref().unwrap().id.to_string()])
                    .await
                    .map_err(storage_error)?;
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                mapped.map_err(storage_error)?
            };
            let body = zeroize::Zeroizing::new(body);
            let existing: ssh::Credential = serde_json::from_str(&body).map_err(storage_error)?;
            let ssh::Credential::KeyPath { passphrase, .. } = existing else {
                return Err(Fault::invalid("SSH credential does not match its profile"));
            };
            Some(ssh::Credential::KeyPath {
                path: Secret::new(path),
                passphrase,
            })
        } else {
            Some(ssh::Credential::KeyPath {
                path: Secret::new(path),
                passphrase: None,
            })
        }
    } else if previous
        .as_ref()
        .is_some_and(|p| p.authentication == authentication)
    {
        None
    } else if authentication == ssh::Authentication::Agent {
        Some(ssh::Credential::Agent)
    } else {
        return Err(Fault::invalid("SSH credential is required"));
    };
    let profile = ssh::Profile {
        id: previous.as_ref().map_or_else(SshId::new, |p| p.id),
        revision: 0,
        sharing: match &previous {
            Some(profile) => profile.sharing.clone(),
            None => sharing(db, turn).await?,
        },
        name: required(
            input
                .name
                .or_else(|| previous.as_ref().map(|p| p.name.clone())),
            "name",
        )?,
        host: required(
            input
                .host
                .or_else(|| previous.as_ref().map(|p| p.host.clone())),
            "host",
        )?,
        port: required(
            input.port.or_else(|| previous.as_ref().map(|p| p.port)),
            "port",
        )?,
        username: required(
            input
                .username
                .or_else(|| previous.as_ref().map(|p| p.username.clone())),
            "username",
        )?,
        authentication,
        host_key: previous.and_then(|p| p.host_key),
    };
    Ok(Command::SaveSsh {
        profile,
        expected_revision,
        credential,
    })
}

fn key_path(path: &str) -> Result<(), Fault> {
    if path.len() > 4096 || path.contains('\0') || !std::path::Path::new(path).is_absolute() {
        return Err(Fault::invalid("SSH key path must be absolute"));
    }
    Ok(())
}

async fn creation(db: &Connection, turn: TurnId) -> Result<(), Fault> {
    super::super::calls::active(db, turn).await?;
    if super::super::permissions::config(db, turn)
        .await?
        .resource
        .is_some()
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "bound conversations cannot create another connection",
        ));
    }
    Ok(())
}

async fn sharing(
    db: &Connection,
    turn: TurnId,
) -> Result<Option<sailry_protocol::connection::Sharing>, Fault> {
    let run = super::super::calls::active(db, turn).await?;
    let session = super::super::super::commands::session(db, run.session).await?;
    Ok(session
        .project
        .map(|project| sailry_protocol::connection::Sharing::Projects(vec![project])))
}
