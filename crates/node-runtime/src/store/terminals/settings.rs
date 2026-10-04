use crate::store::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::{terminal::Settings, *};

pub(crate) fn read(db: &Connection) -> Result<Settings, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM terminal_settings WHERE singleton=1",
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
    settings.validate()?;
    if !settings.shell.is_empty() && !std::path::Path::new(&settings.shell).is_absolute() {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "terminal shell must be an absolute path on the execution Node",
        ));
    }
    if read(db)?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "terminal settings changed",
        ));
    }
    let mut saved = settings.clone();
    saved.revision += 1;
    db.execute("INSERT INTO terminal_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&saved)?]).map_err(storage_error)?;
    let event = Event::TerminalSettingsChanged {
        revision: saved.revision,
    };
    Ok((Output::TerminalSettings(saved), Some(event)))
}
