use super::database::storage_error;
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Stored {
    #[serde(rename = "revision")]
    _revision: u64,
    #[serde(rename = "resume")]
    _resume: bool,
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
            _revision: 0,
            _resume: true,
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
