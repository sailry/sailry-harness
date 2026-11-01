//! Team configuration shares Node transactions and durable command receipts.
//! The Default team is virtual and never stored.
use super::database::{encode, storage_error};
use sailry_protocol::{ErrorCode, Fault, RoleId, TeamId, team};
use turso::{Connection, params};

fn decode(row: &turso::Row) -> Result<team::Profile, Fault> {
    serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?).map_err(storage_error)
}

pub(super) async fn get(db: &Connection, id: TeamId) -> Result<Option<team::Profile>, Fault> {
    let mut statement = db
        .prepare("SELECT body FROM teams WHERE id=?1")
        .await
        .map_err(storage_error)?;
    match statement.query_row([id.to_string()]).await {
        Ok(row) => decode(&row).map(Some),
        Err(turso::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(storage_error(error)),
    }
}

pub(super) async fn list(db: &Connection) -> Result<Vec<team::Profile>, Fault> {
    let mut rows = db
        .query("SELECT body FROM teams ORDER BY key", ())
        .await
        .map_err(storage_error)?;
    let mut profiles = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        profiles.push(decode(&row)?);
    }
    Ok(profiles)
}

fn next(revision: u64) -> Result<u64, Fault> {
    revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("team revision exhausted"))
}

async fn write(db: &Connection, team: &team::Profile) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO teams(id,key,body) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET key=excluded.key,body=excluded.body",
        params![team.id.to_string(), team.key.as_str(), encode(team)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn put(
    db: &Connection,
    team: &team::Profile,
    expected: u64,
) -> Result<team::Profile, Fault> {
    team.validate()?;
    let revision = get(db, team.id)
        .await?
        .map_or(0, |current| current.revision);
    if revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "team revision changed",
        ));
    }
    if db
        .prepare("SELECT EXISTS(SELECT 1 FROM teams WHERE key=?1 AND id<>?2)")
        .await
        .map_err(storage_error)?
        .query_row(params![team.key.as_str(), team.id.to_string()])
        .await
        .map_err(storage_error)?
        .get::<bool>(0)
        .map_err(storage_error)?
    {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "team key is already in use",
        ));
    }
    for role in &team.members {
        if super::roles::get(db, *role).await?.is_none() {
            return Err(Fault::new(ErrorCode::NotFound, "role does not exist"));
        }
    }
    if let Some(selection) = &team.model {
        let provider = super::agent::providers::get(db, selection.provider)
            .await?
            .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "team provider is unavailable"))?;
        super::roles::validate_model(&provider, selection)?;
    }
    let mut team = team.clone();
    team.revision = next(revision)?;
    write(db, &team).await?;
    Ok(team)
}

pub(super) async fn remove(db: &Connection, id: TeamId, expected: u64) -> Result<(), Fault> {
    let team = get(db, id)
        .await?
        .ok_or_else(|| Fault::new(ErrorCode::NotFound, "team does not exist"))?;
    if team.revision != expected {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "team revision changed",
        ));
    }
    db.execute("DELETE FROM teams WHERE id=?1", [id.to_string()])
        .await
        .map_err(storage_error)?;
    Ok(())
}

/// Removes a deleted role from every team, advancing each affected revision.
pub(super) async fn prune(db: &Connection, role: RoleId) -> Result<Vec<team::Profile>, Fault> {
    let mut changed = Vec::new();
    for mut team in list(db).await? {
        let count = team.members.len();
        team.members.retain(|member| *member != role);
        if team.members.len() != count {
            team.revision = next(team.revision)?;
            write(db, &team).await?;
            changed.push(team);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
