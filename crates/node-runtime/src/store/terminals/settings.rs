use crate::store::database::{encode, storage_error};
use sailry_protocol::{terminal::Settings, *};
use turso::Connection;

pub(crate) async fn read(db: &Connection) -> Result<Settings, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM terminal_settings WHERE singleton=1")
            .await
            .map_err(storage_error)?;
        match statement.query_row(()).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    body.map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) async fn save(
    db: &Connection,
    settings: &Settings,
) -> Result<(Output, Option<Event>), Fault> {
    settings.validate()?;
    if !settings.shell.is_empty() && !std::path::Path::new(&settings.shell).is_absolute() {
        return Err(Fault::invalid(
            "terminal shell must be an absolute path on the execution Node",
        ));
    }
    if read(db).await?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "terminal settings changed",
        ));
    }
    let mut saved = settings.clone();
    saved.revision += 1;
    db.execute("INSERT INTO terminal_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", [encode(&saved)?]).await.map_err(storage_error)?;
    let event = Event::TerminalSettingsChanged {
        revision: saved.revision,
    };
    Ok((Output::TerminalSettings(saved), Some(event)))
}
