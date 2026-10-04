//! Profile ownership adapted from sailry-code 67ae9fa0 sailry-connections.
use super::database::{Database, encode, storage_error};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    database::{Connection as Target, Engine, Profile},
    *,
};

pub(super) mod worker;

pub(super) fn list(db: &Connection) -> Result<Vec<Profile>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM database_profiles ORDER BY id")
        .map_err(storage_error)?;
    query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|row| serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error))
        .collect()
}

pub(super) fn read(db: &Connection, id: DatabaseId) -> Result<Option<Profile>, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM database_profiles WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

fn current(db: &Connection, id: DatabaseId, revision: u64) -> Result<Profile, Fault> {
    let profile = read(db, id)?.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotFound,
            "database profile does not exist on this Node",
        )
    })?;
    super::commands::check_revision(profile.revision, revision)?;
    Ok(profile)
}

pub(super) fn execute(
    db: &Connection,
    command: &Command,
) -> Result<(Output, Option<Event>), Fault> {
    match command {
        Command::ListDatabases => Ok((Output::DatabaseProfiles(list(db)?), None)),
        Command::SaveDatabase {
            profile,
            expected_revision,
            password,
        } => {
            let previous = read(db, profile.id)?;
            super::commands::check_revision(
                previous.as_ref().map_or(0, |profile| profile.revision),
                *expected_revision,
            )?;
            super::connections::validate(db, profile.sharing.as_ref())?;
            let mut profile = profile.clone();
            profile.name = profile.name.trim().into();
            if profile.name.is_empty() || profile.name.len() > 256 {
                return Err(invalid("invalid database profile name"));
            }
            validate(&profile.connection, password.as_ref())?;
            validate_ssh(db, &profile.connection)?;
            profile.revision = expected_revision
                .checked_add(1)
                .filter(|revision| *revision <= i64::MAX as u64)
                .ok_or_else(|| invalid("database profile revision exhausted"))?;
            let password = if matches!(profile.connection, Target::Sqlite { .. }) {
                Some("")
            } else {
                password.as_ref().map(Secret::expose)
            };
            if previous.is_none() {
                if list(db)?.len() >= 128 {
                    return Err(Fault::new(
                        ErrorCode::Busy,
                        "database profile capacity exhausted",
                    ));
                }
                db.execute(
                    "INSERT INTO database_profiles(id,body,password) VALUES(?1,?2,?3)",
                    params![
                        profile.id.to_string(),
                        encode(&profile)?,
                        password.unwrap_or("")
                    ],
                )
                .map_err(storage_error)?;
            } else {
                db.execute("UPDATE database_profiles SET body=?2,password=COALESCE(?3,password) WHERE id=?1", params![profile.id.to_string(), encode(&profile)?, password]).map_err(storage_error)?;
            }
            Ok((
                Output::DatabaseProfile(profile.clone()),
                Some(Event::DatabaseChanged(profile)),
            ))
        }
        Command::RemoveDatabase {
            profile,
            expected_revision,
        } => {
            current(db, *profile, *expected_revision)?;
            db.execute(
                "DELETE FROM database_profiles WHERE id=?1",
                [profile.to_string()],
            )
            .map_err(storage_error)?;
            Ok((
                Output::DatabaseProfiles(list(db)?),
                Some(Event::DatabaseRemoved { id: *profile }),
            ))
        }
        _ => Err(invalid("database configuration command expected")),
    }
}

pub(super) fn prepare(
    database: &Database,
    command: &Command,
) -> Result<crate::databases::Operation, Fault> {
    if let Command::TestDatabase {
        profile,
        expected_revision,
        password,
    } = command
    {
        let previous = read(&database.connection, profile.id)?;
        super::commands::check_revision(
            previous.as_ref().map_or(0, |profile| profile.revision),
            *expected_revision,
        )?;
        validate(&profile.connection, password.as_ref())?;
        let password = if let Some(password) = password {
            password.clone()
        } else if previous.is_some() && !matches!(profile.connection, Target::Sqlite { .. }) {
            Secret::new(
                database
                    .connection
                    .query_row(
                        "SELECT password FROM database_profiles WHERE id=?1",
                        [profile.id.to_string()],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(storage_error)?,
            )
        } else {
            Secret::new(String::new())
        };
        return Ok(crate::databases::Operation {
            ssh: prepare_ssh(database, &profile.connection)?,
            profile: profile.clone(),
            password,
            query: None,
            browse: None,
            storage: database.profile.clone(),
        });
    }
    let (id, revision, query) = match command {
        Command::CheckDatabase {
            profile,
            expected_revision,
        } => (*profile, *expected_revision, None),
        Command::BrowseDatabase {
            profile,
            expected_revision,
            ..
        } => (*profile, *expected_revision, None),
        Command::QueryDatabase {
            profile,
            expected_revision,
            sql,
            row_limit,
            timeout_ms,
            ..
        } => {
            if sql.trim().is_empty()
                || sql.len() > 65536
                || !(1..=10_000).contains(row_limit)
                || !(1..=3_600_000).contains(timeout_ms)
            {
                return Err(invalid("invalid database query or limit"));
            }
            (
                *profile,
                *expected_revision,
                Some(crate::databases::Query {
                    sql: sql.clone(),
                    row_limit: *row_limit as usize,
                    timeout_ms: *timeout_ms,
                }),
            )
        }
        _ => return Err(invalid("database operation expected")),
    };
    let mut profile = current(&database.connection, id, revision)?;
    if matches!(
        command,
        Command::QueryDatabase {
            read_only: true,
            ..
        }
    ) {
        profile.read_only = true;
    }
    let selected = match command {
        Command::QueryDatabase { database, .. } | Command::BrowseDatabase { database, .. } => {
            database.as_ref()
        }
        _ => None,
    };
    if let Some(selected) = selected {
        if selected.is_empty() || selected.len() > 256 || selected.chars().any(char::is_control) {
            return Err(invalid("invalid selected database"));
        }
        if let Target::Network { database, .. }
        | Target::Socket { database, .. }
        | Target::Ssh { database, .. } = &mut profile.connection
        {
            *database = selected.clone();
        } else if selected != "main" {
            return Err(invalid("unknown SQLite database"));
        }
    }
    let password = database
        .connection
        .query_row(
            "SELECT password FROM database_profiles WHERE id=?1",
            [id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .map_err(storage_error)?;
    Ok(crate::databases::Operation {
        ssh: prepare_ssh(database, &profile.connection)?,
        profile,
        password: Secret::new(password),
        query,
        browse: match command {
            Command::BrowseDatabase { database, .. } => Some(database.clone()),
            _ => None,
        },
        storage: database.profile.clone(),
    })
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

fn validate(connection: &Target, password: Option<&Secret>) -> Result<(), Fault> {
    match connection {
        Target::Sqlite { path } => {
            if path.len() > 4096 || path.contains('\0') || !std::path::Path::new(path).is_absolute()
            {
                return Err(invalid(
                    "SQLite requires an absolute path on the execution Node",
                ));
            }
            if password.is_some() {
                return Err(invalid("SQLite does not use a network password"));
            }
        }
        Target::Network {
            engine,
            host,
            port,
            database,
            username,
            ..
        }
        | Target::Ssh {
            engine,
            host,
            port,
            database,
            username,
            ..
        } => {
            if host.is_empty()
                || host.len() > 253
                || host.chars().any(|c| c.is_whitespace() || c.is_control())
                || host.contains('/')
            {
                return Err(invalid("invalid database host"));
            }
            validate_network(*engine, *port, database, username, password)?;
        }
        Target::Socket {
            engine,
            path,
            port,
            database,
            username,
        } => {
            if !cfg!(unix) {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "Database sockets are unavailable on this execution Node",
                ));
            }
            if path.len() > 4096 || path.contains('\0') || !std::path::Path::new(path).is_absolute()
            {
                return Err(invalid(
                    "Socket requires an absolute path on the execution Node",
                ));
            }
            validate_network(*engine, *port, database, username, password)?;
        }
    }
    Ok(())
}

fn validate_network(
    engine: Engine,
    port: u16,
    database: &str,
    username: &str,
    password: Option<&Secret>,
) -> Result<(), Fault> {
    if engine == Engine::Sqlite
        || port == 0
        || database.len() > 256
        || username.is_empty()
        || username.len() > 256
        || username.chars().any(char::is_control)
        || database.chars().any(char::is_control)
        || password.is_some_and(|p| p.expose().len() > 65536)
    {
        return Err(invalid("invalid network database profile"));
    }
    Ok(())
}

fn validate_ssh(db: &Connection, connection: &Target) -> Result<Option<ssh::Profile>, Fault> {
    let Target::Ssh { ssh, .. } = connection else {
        return Ok(None);
    };
    super::ssh::read(db, *ssh)?.map(Some).ok_or_else(|| {
        Fault::new(
            ErrorCode::NotFound,
            "SSH connection does not exist on this Node",
        )
    })
}

fn prepare_ssh(
    database: &Database,
    connection: &Target,
) -> Result<Option<crate::databases::Ssh>, Fault> {
    let Some(profile) = validate_ssh(&database.connection, connection)? else {
        return Ok(None);
    };
    let operation = super::ssh::prepare(
        database,
        &Command::CheckSsh {
            profile: profile.id,
            expected_revision: profile.revision,
        },
    )?;
    Ok(Some(crate::databases::Ssh {
        profile: operation.profile,
        credential: operation.credential,
    }))
}
