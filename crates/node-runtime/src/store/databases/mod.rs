//! Profile ownership adapted from sailry-code 67ae9fa0 sailry-connections.
use super::database::{Database, encode, storage_error};
use sailry_protocol::{
    database::{Connection as Target, Engine, Profile},
    *,
};
use turso::{Connection, params};

pub(in crate::store) mod credentials;
pub(super) mod worker;

pub(super) async fn list(db: &Connection) -> Result<Vec<Profile>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM database_profiles ORDER BY id")
        .await
        .map_err(storage_error)?;
    {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|row| serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error))
    .collect()
}

pub(super) async fn read(db: &Connection, id: DatabaseId) -> Result<Option<Profile>, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM database_profiles WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
}

async fn current(db: &Connection, id: DatabaseId, revision: u64) -> Result<Profile, Fault> {
    let profile = read(db, id).await?.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotFound,
            "database profile does not exist on this Node",
        )
    })?;
    super::commands::check_revision(profile.revision, revision)?;
    Ok(profile)
}

pub(super) async fn execute(
    db: &Connection,
    command: &Command,
) -> Result<(Output, Option<Event>), Fault> {
    match command {
        Command::ListDatabases => Ok((Output::DatabaseProfiles(list(db).await?), None)),
        Command::SaveDatabase {
            profile,
            expected_revision,
            password,
        } => {
            let previous = read(db, profile.id).await?;
            super::commands::check_revision(
                previous.as_ref().map_or(0, |profile| profile.revision),
                *expected_revision,
            )?;
            super::connections::validate(db, profile.sharing.as_ref()).await?;
            let mut profile = profile.clone();
            profile.name = profile.name.trim().into();
            if profile.name.is_empty() || profile.name.len() > 256 {
                return Err(sailry_protocol::Fault::invalid(
                    "invalid database profile name",
                ));
            }
            validate(&profile.connection, password.as_ref())?;
            validate_ssh(db, &profile.connection).await?;
            profile.revision = expected_revision
                .checked_add(1)
                .filter(|revision| *revision <= i64::MAX as u64)
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("database profile revision exhausted")
                })?;
            if previous.is_some()
                && password.is_none()
                && !matches!(profile.connection, Target::Sqlite { .. })
            {
                credentials::check(db, profile.id, &profile.connection).await?;
            }
            let target = credentials::capture(db, &profile.connection).await?;
            let password = if matches!(profile.connection, Target::Sqlite { .. }) {
                Some("")
            } else {
                password.as_ref().map(Secret::expose)
            };
            if previous.is_none() {
                if list(db).await?.len() >= 128 {
                    return Err(Fault::new(
                        ErrorCode::Busy,
                        "database profile capacity exhausted",
                    ));
                }
                db.execute(
                    "INSERT INTO database_profiles(id,body,password,password_target) VALUES(?1,?2,?3,?4)",
                    params![
                        profile.id.to_string(),
                        encode(&profile)?,
                        password.unwrap_or(""),
                        target
                    ],
                ).await
                .map_err(storage_error)?;
            } else {
                db.execute("UPDATE database_profiles SET body=?2,password=COALESCE(?3,password),password_target=?4 WHERE id=?1", params![profile.id.to_string(), encode(&profile)?, password, target]).await.map_err(storage_error)?;
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
            current(db, *profile, *expected_revision).await?;
            db.execute(
                "DELETE FROM database_profiles WHERE id=?1",
                [profile.to_string()],
            )
            .await
            .map_err(storage_error)?;
            Ok((
                Output::DatabaseProfiles(list(db).await?),
                Some(Event::DatabaseRemoved { id: *profile }),
            ))
        }
        _ => Err(sailry_protocol::Fault::invalid(
            "database configuration command expected",
        )),
    }
}

pub(super) async fn prepare(
    database: &Database,
    command: &Command,
) -> Result<crate::databases::Operation, Fault> {
    if let Command::TestDatabase {
        profile,
        expected_revision,
        password,
    } = command
    {
        let previous = read(&database.connection, profile.id).await?;
        super::commands::check_revision(
            previous.as_ref().map_or(0, |profile| profile.revision),
            *expected_revision,
        )?;
        validate(&profile.connection, password.as_ref())?;
        let password = if let Some(password) = password {
            password.clone()
        } else if previous.is_some() && !matches!(profile.connection, Target::Sqlite { .. }) {
            credentials::check(&database.connection, profile.id, &profile.connection).await?;
            Secret::new({
                let mut statement = database
                    .connection
                    .prepare("SELECT password FROM database_profiles WHERE id=?1")
                    .await
                    .map_err(storage_error)?;
                let row = statement
                    .query_row([profile.id.to_string()])
                    .await
                    .map_err(storage_error)?;
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);
                mapped.map_err(storage_error)?
            })
        } else {
            Secret::new(String::new())
        };
        return Ok(crate::databases::Operation {
            ssh: prepare_ssh(database, &profile.connection).await?,
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
                return Err(sailry_protocol::Fault::invalid(
                    "invalid database query or limit",
                ));
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
        _ => {
            return Err(sailry_protocol::Fault::invalid(
                "database operation expected",
            ));
        }
    };
    let mut profile = current(&database.connection, id, revision).await?;
    if matches!(
        command,
        Command::QueryDatabase {
            read_only: true,
            ..
        }
    ) {
        profile.read_only = true;
    }
    if !matches!(profile.connection, Target::Sqlite { .. }) {
        credentials::check(&database.connection, id, &profile.connection).await?;
    }
    let selected = match command {
        Command::QueryDatabase { database, .. } | Command::BrowseDatabase { database, .. } => {
            database.as_ref()
        }
        _ => None,
    };
    if let Some(selected) = selected {
        if selected.is_empty() || selected.len() > 256 || selected.chars().any(char::is_control) {
            return Err(sailry_protocol::Fault::invalid("invalid selected database"));
        }
        if let Target::Network { database, .. }
        | Target::Socket { database, .. }
        | Target::Ssh { database, .. } = &mut profile.connection
        {
            *database = selected.clone();
        } else if selected != "main" {
            return Err(sailry_protocol::Fault::invalid("unknown SQLite database"));
        }
    }
    let password = {
        let mut statement = database
            .connection
            .prepare("SELECT password FROM database_profiles WHERE id=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<String>(0))(&row);
        mapped.map_err(storage_error)?
    };
    Ok(crate::databases::Operation {
        ssh: prepare_ssh(database, &profile.connection).await?,
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

fn validate(connection: &Target, password: Option<&Secret>) -> Result<(), Fault> {
    match connection {
        Target::Sqlite { path } => {
            if path.len() > 4096 || path.contains('\0') || !std::path::Path::new(path).is_absolute()
            {
                return Err(sailry_protocol::Fault::invalid(
                    "SQLite requires an absolute path on the execution Node",
                ));
            }
            if password.is_some() {
                return Err(sailry_protocol::Fault::invalid(
                    "SQLite does not use a network password",
                ));
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
                return Err(sailry_protocol::Fault::invalid("invalid database host"));
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
                return Err(sailry_protocol::Fault::invalid(
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
        return Err(sailry_protocol::Fault::invalid(
            "invalid network database profile",
        ));
    }
    Ok(())
}

async fn validate_ssh(db: &Connection, connection: &Target) -> Result<Option<ssh::Profile>, Fault> {
    let Target::Ssh { ssh, .. } = connection else {
        return Ok(None);
    };
    super::ssh::read(db, *ssh).await?.map(Some).ok_or_else(|| {
        Fault::new(
            ErrorCode::NotFound,
            "SSH connection does not exist on this Node",
        )
    })
}

async fn prepare_ssh(
    database: &Database,
    connection: &Target,
) -> Result<Option<crate::databases::Ssh>, Fault> {
    let Some(profile) = validate_ssh(&database.connection, connection).await? else {
        return Ok(None);
    };
    let operation = super::ssh::prepare(
        database,
        &Command::CheckSsh {
            profile: profile.id,
            expected_revision: profile.revision,
        },
    )
    .await?;
    Ok(Some(crate::databases::Ssh {
        profile: operation.profile,
        credential: operation.credential,
    }))
}
