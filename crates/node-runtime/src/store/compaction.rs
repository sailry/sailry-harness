//! Persist the default policy and retain each admitted turn's percentage.
use crate::store::database::{encode, storage_error};
use sailry_protocol::{
    ErrorCode, Event, Fault, Output, TurnId, conversation::compaction::Settings,
};
use turso::Connection;

pub(super) async fn read(db: &Connection) -> Result<Settings, Fault> {
    let mut query = db
        .prepare("SELECT body FROM compaction_settings WHERE singleton=1")
        .await
        .map_err(storage_error)?;
    let settings: Settings = match query.query_row(()).await {
        Ok(row) => serde_json::from_slice(&row.get::<Vec<u8>>(0).map_err(storage_error)?)
            .map_err(storage_error)?,
        Err(turso::Error::QueryReturnedNoRows) => Settings::default(),
        Err(error) => return Err(storage_error(error)),
    };
    settings.validate()?;
    Ok(settings)
}

pub(super) async fn save(
    db: &Connection,
    settings: &Settings,
) -> Result<(Output, Option<Event>), Fault> {
    settings.validate()?;
    if read(db).await?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "compaction settings changed",
        ));
    }
    let revision = settings
        .revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| Fault::invalid("compaction settings revision exhausted"))?;
    let saved = Settings {
        revision,
        ..*settings
    };
    db.execute("INSERT INTO compaction_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&saved)?])
        .await.map_err(storage_error)?;
    Ok((
        Output::CompactionSettings(saved),
        Some(Event::CompactionSettingsChanged { revision }),
    ))
}

pub(super) async fn captured(db: &Connection, turn: TurnId) -> Result<u8, Fault> {
    let mut query = db
        .prepare("SELECT compaction_percent FROM agent_runs WHERE turn=?1")
        .await
        .map_err(storage_error)?;
    let row = query
        .query_row([turn.to_string()])
        .await
        .map_err(storage_error)?;
    let value = row.get::<i64>(0).map_err(storage_error)?;
    let percent = u8::try_from(value).map_err(storage_error)?;
    Settings {
        percent,
        ..Settings::default()
    }
    .validate()?;
    Ok(percent)
}
