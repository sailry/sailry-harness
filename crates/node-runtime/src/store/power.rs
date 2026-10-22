//! Authoritative Node policy; native assertions and execution counts are never stored.
use crate::store::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::{power::Settings, *};

pub(crate) fn read(db: &Connection) -> Result<Settings, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM power_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn save(db: &Connection, settings: &Settings) -> Result<(Output, Option<Event>), Fault> {
    if read(db)?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "power settings changed",
        ));
    }
    let mut saved = settings.clone();
    saved.revision = saved
        .revision
        .checked_add(1)
        .ok_or_else(|| Fault::invalid("power settings revision exhausted"))?;
    db.execute("INSERT INTO power_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&saved)?]).map_err(storage_error)?;
    let event = Event::PowerSettingsChanged {
        revision: saved.revision,
    };
    Ok((Output::PowerSettings(saved), Some(event)))
}

pub(super) fn failed(
    database: &mut super::database::Database,
    code: i64,
    events: &tokio::sync::broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let tx = database.connection.transaction().map_err(storage_error)?;
    let event = super::notifications::system::power(&tx, code)?;
    let envelopes = super::database::record(&tx, database.node, vec![event])?;
    tx.commit().map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(())
}
