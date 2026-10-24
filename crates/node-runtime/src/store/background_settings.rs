use super::database::storage_error;
use sailry_protocol::*;
use serde::{Deserialize, Serialize};
use turso::Connection;

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

async fn stored(db: &Connection) -> Result<Stored, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM background_settings WHERE singleton=1")
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

pub(super) async fn retained(db: &Connection) -> Result<Option<u64>, Fault> {
    Ok(stored(db).await?.child_concurrency)
}
