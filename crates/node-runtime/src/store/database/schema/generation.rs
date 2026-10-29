//! A missing storage generation authorizes this development baseline's one-time rebuild.
use crate::Error;
use std::path::Path;
use turso::Connection;

#[cfg(test)]
mod tests;

const CURRENT: i64 = 1;

pub(super) async fn current(connection: &Connection, path: &Path) -> Result<bool, Error> {
    let mut columns = connection.query("PRAGMA table_info(node)", ()).await?;
    let mut declared = false;
    while let Some(column) = columns.next().await? {
        if column.get::<String>(1)? == "storage_generation" {
            declared = true;
            break;
        }
    }
    drop(columns);
    if !declared {
        return Ok(false);
    }
    let current = connection
        .prepare(
            "SELECT CASE WHEN typeof(storage_generation)='integer' AND storage_generation=?1 THEN 1 ELSE 0 END FROM node WHERE singleton=1",
        )
        .await?
        .query_row([CURRENT])
        .await?
        .get::<bool>(0)?;
    if !current {
        return Err(super::incompatible(
            path,
            "node.storage_generation is invalid or unsupported",
        ));
    }
    Ok(true)
}
