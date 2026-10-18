use super::database::{encode, storage_error};
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::{conversation::background::Settings, *};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Stored {
    revision: u64,
    resume: bool,
    // Retained solely for migration into a declared package-owned setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    child_concurrency: Option<u64>,
}

fn stored(db: &Connection) -> Result<Stored, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM background_settings WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let stored = body
        .map(|body| serde_json::from_slice::<Stored>(&body).map_err(storage_error))
        .transpose()?
        .unwrap_or(Stored {
            revision: 0,
            resume: true,
            child_concurrency: None,
        });
    if stored.child_concurrency == Some(0) {
        return Err(storage_error("invalid retained child concurrency"));
    }
    Ok(stored)
}

pub(super) fn retained(db: &Connection) -> Result<Option<u64>, Fault> {
    Ok(stored(db)?.child_concurrency)
}

pub(super) fn read(db: &Connection) -> Result<Settings, Fault> {
    let stored = stored(db)?;
    Ok(Settings {
        revision: stored.revision,
        resume: stored.resume,
    })
}

pub(super) fn save(db: &Connection, settings: &Settings) -> Result<(Output, Option<Event>), Fault> {
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
    };
    db.execute(
        "INSERT INTO background_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body",
        [encode(&Stored { revision: saved.revision, resume: saved.resume, child_concurrency: retained(db)? })?],
    ).map_err(storage_error)?;
    Ok((
        Output::BackgroundSettings(saved.clone()),
        Some(Event::BackgroundSettingsChanged {
            revision: saved.revision,
        }),
    ))
}
