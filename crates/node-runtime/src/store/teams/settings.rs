//! The execution Node owns the delegation budget; claims read it when starting children.
use crate::store::database::{encode, storage_error};
use sailry_protocol::{ErrorCode, Event, Fault, Output, team::Settings};
use turso::Connection;

#[cfg(test)]
mod tests;

async fn stored(db: &Connection) -> Result<Option<Settings>, Fault> {
    let mut statement = db
        .prepare("SELECT body FROM team_settings WHERE singleton=1")
        .await
        .map_err(storage_error)?;
    let body: Vec<u8> = match statement.query_row(()).await {
        Ok(row) => row.get(0).map_err(storage_error)?,
        Err(turso::Error::QueryReturnedNoRows) => return Ok(None),
        Err(error) => return Err(storage_error(error)),
    };
    let settings: Settings = serde_json::from_slice(&body).map_err(storage_error)?;
    settings.validate().map_err(storage_error)?;
    Ok(Some(settings))
}

/// Whether the Node already stores an explicit or imported budget.
pub(in crate::store) async fn configured(db: &Connection) -> Result<bool, Fault> {
    Ok(stored(db).await?.is_some())
}

pub(in crate::store) async fn read(db: &Connection) -> Result<Settings, Fault> {
    Ok(stored(db).await?.unwrap_or_default())
}

async fn write(db: &Connection, settings: &Settings) -> Result<(), Fault> {
    db.execute(
        "INSERT INTO team_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",
        [encode(settings)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(in crate::store) async fn save(
    db: &Connection,
    settings: &Settings,
) -> Result<(Output, Option<Event>), Fault> {
    settings.validate()?;
    if read(db).await?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "team settings changed",
        ));
    }
    let mut saved = settings.clone();
    saved.revision = saved
        .revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("team settings revision exhausted"))?;
    write(db, &saved).await?;
    let event = Event::TeamSettingsChanged {
        revision: saved.revision,
    };
    Ok((Output::TeamSettings(saved), Some(event)))
}

/// One-time import of a budget configured before the Node owned it. An existing
/// Node setting always wins, so restarting never overwrites a later choice.
pub(in crate::store) async fn import(db: &Connection, child_concurrency: u64) -> Result<(), Fault> {
    if configured(db).await? {
        return Ok(());
    }
    let settings = Settings {
        revision: 1,
        child_concurrency,
    };
    settings.validate()?;
    write(db, &settings).await
}
