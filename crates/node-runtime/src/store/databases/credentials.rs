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

async fn binding(db: &Connection, connection: &Target) -> Result<Binding, Fault> {
    let tunnel = if let Target::Ssh { ssh, .. } = connection {
        let profile = super::super::ssh::read(db, *ssh).await?.ok_or_else(|| {
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

pub(super) async fn capture(db: &Connection, connection: &Target) -> Result<Vec<u8>, Fault> {
    encode(&binding(db, connection).await?)
}

pub(super) async fn check(
    db: &Connection,
    id: DatabaseId,
    connection: &Target,
) -> Result<(), Fault> {
    let stored: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT password_target FROM database_profiles WHERE id=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    let expected = binding(db, connection).await?;
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
pub(in crate::store) async fn reconcile(db: &Connection) -> Result<bool, Fault> {
    let mut changed = false;
    for profile in list(db).await? {
        if matches!(profile.connection, Target::Ssh { .. }) {
            continue;
        }
        let target = capture(db, &profile.connection).await?;
        changed |= db.execute(
            "UPDATE database_profiles SET password_target=?2 WHERE id=?1 AND password_target IS NULL",
            params![profile.id.to_string(),target],
        ).await.map_err(storage_error)? != 0;
    }
    Ok(changed)
}

pub(in crate::store) async fn validate(db: &Connection) -> Result<(), Fault> {
    let mut query = db
        .prepare("SELECT password_target FROM database_profiles WHERE password_target IS NOT NULL")
        .await
        .map_err(storage_error)?;
    {
        let mut rows = query.query(()).await.map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let row: turso::Result<_> = row.get::<Vec<u8>>(0);

            let _: Binding =
                serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
