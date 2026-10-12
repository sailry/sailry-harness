//! Conversation connection edits contain public configuration and preserve stored credentials.
use super::*;
use serde::Deserialize;

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

pub(super) fn database(
    db: &Connection,
    turn: TurnId,
    package: &plugin::Reference,
    captured: &[Resource],
    input: DatabaseInput,
) -> Result<Command, Fault> {
    let expected_revision = revision(input.connection, input.expected_revision)?;
    let previous = input
        .connection
        .map(|id| {
            check(db, turn, package, Resource::Database(id), captured)?;
            super::super::super::databases::read(db, id)?.ok_or_else(|| {
                Fault::new(ErrorCode::NotFound, "database connection does not exist")
            })
        })
        .transpose()?;
    if previous.is_none() {
        creation(db, turn)?;
    }
    let mut profile = previous.clone().unwrap_or(database::Profile {
        id: DatabaseId::new(),
        revision: 0,
        sharing: None,
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
                    path: required(input.path.or(existing), "path")?,
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

pub(super) fn ssh(
    db: &Connection,
    turn: TurnId,
    package: &plugin::Reference,
    captured: &[Resource],
    input: SshInput,
) -> Result<Command, Fault> {
    let expected_revision = revision(input.connection, input.expected_revision)?;
    let previous = input
        .connection
        .map(|id| {
            check(db, turn, package, Resource::Ssh(id), captured)?;
            super::super::super::ssh::read(db, id)?
                .ok_or_else(|| Fault::new(ErrorCode::NotFound, "SSH connection does not exist"))
        })
        .transpose()?;
    if previous.is_none() {
        creation(db, turn)?;
    }
    let authentication = required(
        input
            .authentication
            .or_else(|| previous.as_ref().map(|p| p.authentication)),
        "authentication",
    )?;
    if input.authentication.is_some_and(|value| {
        !matches!(
            value,
            ssh::Authentication::Agent | ssh::Authentication::KeyPath
        )
    }) {
        return Err(Fault::invalid(
            "conversation SSH authentication must use an agent or key path",
        ));
    }
    let credential = if let Some(path) = input.key_path {
        if authentication != ssh::Authentication::KeyPath
            || path.len() > 4096
            || path.contains('\0')
            || !std::path::Path::new(&path).is_absolute()
        {
            return Err(Fault::invalid(
                "SSH key path requires an absolute path and key_path authentication",
            ));
        }
        if previous
            .as_ref()
            .is_some_and(|p| p.authentication == ssh::Authentication::KeyPath)
        {
            // A path edit must retain the existing optional passphrase inside the Node.
            let body: String = db
                .query_row(
                    "SELECT credential FROM ssh_profiles WHERE id=?1",
                    [previous.as_ref().unwrap().id.to_string()],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
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
        return Err(Fault::invalid(
            "SSH connection requires an explicit key path",
        ));
    };
    let profile = ssh::Profile {
        id: previous.as_ref().map_or_else(SshId::new, |p| p.id),
        revision: 0,
        sharing: previous.as_ref().and_then(|p| p.sharing.clone()),
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

fn creation(db: &Connection, turn: TurnId) -> Result<(), Fault> {
    super::super::calls::active(db, turn)?;
    if super::super::permissions::config(db, turn)?
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
