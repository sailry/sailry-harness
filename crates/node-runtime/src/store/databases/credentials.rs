//! Password reuse follows the actual database and SSH destination, not a profile ID.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    connection: Target,
    tunnel: Option<Tunnel>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tunnel {
    host: String,
    port: u16,
    username: String,
    authentication: ssh::Authentication,
}

fn binding(db: &Connection, connection: &Target) -> Result<Binding, Fault> {
    let tunnel = if let Target::Ssh { ssh, .. } = connection {
        let profile = super::super::ssh::read(db, *ssh)?.ok_or_else(|| {
            Fault::new(ErrorCode::NotConfigured, "database tunnel is unavailable")
        })?;
        Some(Tunnel {
            host: profile.host,
            port: profile.port,
            username: profile.username,
            authentication: profile.authentication,
        })
    } else {
        None
    };
    Ok(Binding {
        connection: connection.clone(),
        tunnel,
    })
}

pub(super) fn capture(db: &Connection, connection: &Target) -> Result<Vec<u8>, Fault> {
    encode(&binding(db, connection)?)
}

pub(super) fn check(db: &Connection, id: DatabaseId, connection: &Target) -> Result<(), Fault> {
    let stored: Option<Vec<u8>> = db
        .query_row(
            "SELECT password_target FROM database_profiles WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let expected = binding(db, connection)?;
    let current: Option<Binding> = stored
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(storage_error)?;
    if current.as_ref() != Some(&expected) {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "database credential target changed; enter a password",
        ));
    }
    Ok(())
}

/// A legacy direct connection already records its destination. An SSH ID does
/// not record the former tunnel authority, so its password stays stored but unbound.
pub(in crate::store) fn reconcile(db: &Connection) -> Result<bool, Fault> {
    let mut changed = false;
    for profile in list(db)? {
        if matches!(profile.connection, Target::Ssh { .. }) {
            continue;
        }
        let target = capture(db, &profile.connection)?;
        changed |= db.execute(
            "UPDATE database_profiles SET password_target=?2 WHERE id=?1 AND password_target IS NULL",
            params![profile.id.to_string(),target],
        ).map_err(storage_error)? != 0;
    }
    Ok(changed)
}

pub(in crate::store) fn validate(db: &Connection) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT password_target FROM database_profiles WHERE password_target IS NOT NULL")
        .map_err(storage_error)?;
    for row in query
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
    {
        let _: Binding =
            serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
