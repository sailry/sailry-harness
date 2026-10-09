use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::{process::Access, sandbox::Settings, *};

pub(super) fn read(db: &Connection) -> Result<Settings, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM sandbox_settings WHERE singleton=1",
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
    if read(db)?.revision != settings.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "command sandbox settings changed",
        ));
    }
    let saved = Settings {
        revision: settings
            .revision
            .checked_add(1)
            .ok_or_else(|| Fault::invalid("command sandbox revision is exhausted"))?,
        enabled: settings.enabled,
    };
    db.execute(
        "INSERT INTO sandbox_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",
        [encode(&saved)?],
    ).map_err(storage_error)?;
    Ok((
        Output::SandboxSettings(saved.clone()),
        Some(Event::SandboxSettingsChanged {
            revision: saved.revision,
        }),
    ))
}

pub(in crate::store) fn effective(
    db: &Connection,
    arguments: &serde_json::Value,
) -> Result<Access, Fault> {
    let requested = arguments
        .get("access")
        .filter(|value| !value.is_null())
        .map(|value| {
            serde_json::from_value(value.clone())
                .map_err(|error| Fault::invalid(format!("invalid command access: {error}")))
        })
        .transpose()?
        .unwrap_or_default();
    Ok(if read(db)?.enabled {
        requested
    } else {
        Access::Unrestricted
    })
}
