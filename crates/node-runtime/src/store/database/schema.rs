//! Initialize fresh databases and validate the single current schema without repairs.
mod definition;
#[cfg(test)]
mod tests;

use crate::Error;
use definition::{Table, normalize};
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::NodeId;
use std::{collections::BTreeMap, path::Path};

const DDL: &str = include_str!("../schema.sql");

pub(super) enum Schema {
    Fresh,
    Current,
}

impl Schema {
    pub(super) fn initialize(&self, connection: &Connection) -> Result<(), Error> {
        if matches!(self, Self::Fresh) {
            connection.execute_batch(DDL)?;
            crate::store::roles::initialize(connection)?;
        }
        Ok(())
    }
}

pub(super) fn prepare(connection: &Connection, path: &Path, node: NodeId) -> Result<Schema, Error> {
    if ownership(connection, path)? {
        return Ok(Schema::Fresh);
    }
    verify(connection, path, DDL)?;
    super::validation::stored(connection)?;
    let identity: Option<Vec<u8>> = connection
        .query_row("SELECT identity FROM node WHERE singleton=1", [], |row| {
            row.get(0)
        })
        .optional()?;
    if identity.as_deref() != Some(node.0.as_slice()) {
        return Err(incompatible(
            path,
            "Node identity is missing or does not match its database",
        ));
    }
    Ok(Schema::Current)
}

fn ownership(connection: &Connection, path: &Path) -> Result<bool, Error> {
    let application: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let objects: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name NOT GLOB 'sqlite_*'",
        [],
        |row| row.get(0),
    )?;
    if application == 0 && version == 0 && objects == 0 {
        return Ok(true);
    }
    if application != super::APPLICATION_ID {
        return Err(incompatible(
            path,
            format!(
                "application_id is {application:#x}, expected {:#x}",
                super::APPLICATION_ID
            ),
        ));
    }
    if version != 1 {
        return Err(incompatible(
            path,
            format!("user_version is {version}, expected 1"),
        ));
    }
    Ok(false)
}

fn verify(connection: &Connection, path: &Path, ddl: &str) -> Result<(), Error> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(ddl)?;
    let mut expected = objects(&reference)?;
    let actual = objects(connection)?;
    compare(&expected, &actual, path)?;
    if expected.contains_key(&("table".into(), "plugin_values".into())) {
        let definitions = crate::store::plugins::storage::search::definitions(connection)
            .map_err(|_| incompatible(path, "plugin search declarations cannot be read"))?;
        for definition in definitions {
            reference.execute_batch(&definition)?;
        }
        let indexes = objects(&reference)?
            .into_iter()
            .filter(|(identity, _)| !expected.contains_key(identity))
            .collect::<BTreeMap<_, _>>();
        compare(&indexes, &actual, path)?;
        expected.extend(indexes);
    }
    if let Some((kind, name)) = actual
        .keys()
        .find(|identity| !expected.contains_key(*identity))
    {
        return Err(incompatible(
            path,
            format!("schema object {name:?} ({kind}) is not part of the current schema"),
        ));
    }
    Ok(())
}

fn compare(
    expected: &BTreeMap<(String, String), String>,
    actual: &BTreeMap<(String, String), String>,
    path: &Path,
) -> Result<(), Error> {
    for (identity, sql) in expected {
        let (kind, name) = identity;
        let Some(actual_sql) = actual.get(identity) else {
            return Err(incompatible(
                path,
                format!("schema object {name:?} is missing"),
            ));
        };
        if normalize(actual_sql) == normalize(sql) {
            continue;
        }
        if kind == "table" {
            let wanted = Table::parse(sql)?;
            let stored = Table::parse(actual_sql)?;
            for (column, declaration) in &wanted.columns {
                let Some(existing) = stored.columns.get(column) else {
                    return Err(incompatible(
                        path,
                        format!("column {name}.{column} is missing"),
                    ));
                };
                if normalize(existing) != normalize(declaration) {
                    return Err(incompatible(
                        path,
                        format!("column {name}.{column} does not match the current definition"),
                    ));
                }
            }
            if let Some(column) = stored
                .columns
                .keys()
                .find(|column| !wanted.columns.contains_key(*column))
            {
                return Err(incompatible(
                    path,
                    format!("column {name}.{column} is not part of the current schema"),
                ));
            }
            if wanted.constraints != stored.constraints {
                return Err(incompatible(
                    path,
                    format!("table {name:?} does not match the current constraints"),
                ));
            }
        }
        return Err(incompatible(
            path,
            format!("schema object {name:?} does not match the current definition"),
        ));
    }
    Ok(())
}

fn objects(connection: &Connection) -> Result<BTreeMap<(String, String), String>, Error> {
    let mut query = connection.prepare(
        "SELECT name,type,sql FROM sqlite_master WHERE name NOT GLOB 'sqlite_*' AND sql IS NOT NULL",
    )?;
    Ok(query
        .query_map([], |row| Ok(((row.get(1)?, row.get(0)?), row.get(2)?)))?
        .collect::<Result<_, _>>()?)
}

fn incompatible(path: &Path, reason: impl std::fmt::Display) -> Error {
    Error::Worker(format!(
        "incompatible development database at {path:?}: {reason}; existing data was not changed; use --data-dir <new absolute private directory> to start with a fresh profile"
    ))
}
