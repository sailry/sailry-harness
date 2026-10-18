use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::{conversation::background::Settings, *};

pub(super) fn read(db: &Connection) -> Result<Settings, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM background_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(super) fn save(db: &Connection, settings: &Settings) -> Result<(Output, Option<Event>), Fault> {
    if settings.child_concurrency == 0 {
        return Err(Fault::invalid("child concurrency must be positive"));
    }
    if read(db)?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "background continuation settings changed",
        ));
    }
    let saved = Settings {
        revision: settings
            .revision
            .checked_add(1)
            .ok_or_else(|| Fault::invalid("background continuation revision is exhausted"))?,
        resume: settings.resume,
        child_concurrency: settings.child_concurrency,
    };
    db.execute(
        "INSERT INTO background_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",
        [encode(&saved)?],
    ).map_err(storage_error)?;
    Ok((
        Output::BackgroundSettings(saved.clone()),
        Some(Event::BackgroundSettingsChanged {
            revision: saved.revision,
        }),
    ))
}
