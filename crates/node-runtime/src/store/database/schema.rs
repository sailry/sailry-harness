//! Upgrade owned databases to the current schema without discarding stored data.
mod backup;
mod definition;
mod plan;
#[cfg(test)]
mod tests;
mod upgrades;

use crate::Error;
use definition::{Table, normalize};
use rusqlite::{Connection, OptionalExtension};
use sailry_protocol::NodeId;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const DDL: &str = include_str!("../schema.sql");

pub(super) struct Schema(State);

enum State {
    Fresh,
    Current,
    Upgrade { plan: plan::Plan, path: PathBuf },
}

impl Schema {
    pub(super) fn initialize(&self, connection: &Connection) -> Result<(), Error> {
        match &self.0 {
            State::Fresh => {
                connection.execute_batch(DDL)?;
                crate::store::roles::initialize(connection)?;
            }
            State::Current => {}
            State::Upgrade { plan, path } => {
                plan.apply(connection)?;
                verify_owned(connection, path, DDL)?;
                upgrades::reconcile(connection)?;
                verify(connection, path, DDL)?;
                super::validation::stored(connection)?;
            }
        }
        Ok(())
    }
}

pub(super) fn prepare(connection: &Connection, path: &Path, node: NodeId) -> Result<Schema, Error> {
    if ownership(connection, path)? {
        return Ok(Schema(State::Fresh));
    }
    identity(connection, path, node)?;
    if verify(connection, path, DDL).is_ok()
        && super::validation::stored(connection).is_ok()
        && crate::store::plugins::storage::search::validate(connection).is_ok()
    {
        return Ok(Schema(State::Current));
    }
    let mut preview = Connection::open_in_memory()?;
    backup::copy(connection, &mut preview)?;
    let plan = plan::Plan::prepare(&preview, path)?;
    plan.apply(&preview)?;
    verify_owned(&preview, path, DDL)?;
    upgrades::reconcile(&preview)?;
    verify(&preview, path, DDL)?;
    super::validation::stored(&preview)?;
    identity(&preview, path, node)?;
    backup::save(connection, path)?;
    Ok(Schema(State::Upgrade {
        plan,
        path: path.to_owned(),
    }))
}

fn identity(connection: &Connection, path: &Path, node: NodeId) -> Result<(), Error> {
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
    Ok(())
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
    layout(connection, path, ddl, false)
}

fn verify_owned(connection: &Connection, path: &Path, ddl: &str) -> Result<(), Error> {
    layout(connection, path, ddl, true)
}

fn layout(connection: &Connection, path: &Path, ddl: &str, rebuilding: bool) -> Result<(), Error> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(ddl)?;
    let mut expected = objects(&reference)?;
    let actual = objects(connection)?;
    compare(&expected, &actual, path)?;
    if expected.contains_key(&("table".into(), "plugin_values".into())) {
        let definitions = crate::store::plugins::storage::search::definitions(connection)
            .map_err(|_| incompatible(path, "plugin search declarations cannot be read"))?;
        for definition in definitions {
            if rebuilding {
                let header = normalize(definition.split_once('(').unwrap().0);
                let name = header
                    .strip_prefix("CREATE VIRTUAL TABLE")
                    .and_then(|header| header.strip_suffix("USING FTS5"))
                    .unwrap()
                    .trim();
                if let Some(sql) = actual.get(&("table".into(), name.to_ascii_lowercase()))
                    && !normalize(sql).starts_with(&format!("{header}("))
                {
                    return Err(incompatible(
                        path,
                        "plugin search table is not an owned FTS5 index",
                    ));
                }
            }
            reference.execute_batch(&definition)?;
        }
        let indexes = objects(&reference)?
            .into_iter()
            .filter(|(identity, _)| !expected.contains_key(identity))
            .collect::<BTreeMap<_, _>>();
        if !rebuilding {
            compare(&indexes, &actual, path)?;
        }
        expected.extend(indexes);
    }
    if let Some((kind, name)) = actual.keys().find(|identity| {
        !expected.contains_key(*identity) && !upgrades::retained(&identity.0, &identity.1)
    }) {
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
                if self::column(name, column, existing) != self::column(name, column, declaration) {
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
            if wanted.header != stored.header
                || wanted.constraints != stored.constraints
                || wanted.options != stored.options
            {
                return Err(incompatible(
                    path,
                    format!("table {name:?} does not match the current constraints"),
                ));
            }
            continue;
        }
        return Err(incompatible(
            path,
            format!("schema object {name:?} does not match the current definition"),
        ));
    }
    Ok(())
}

fn column(table: &str, name: &str, declaration: &str) -> String {
    let normalized = normalize(declaration);
    if upgrades::column(table, name).is_none() {
        return normalized;
    }
    let Some((definition, literal)) = normalized.rsplit_once("DEFAULT X'") else {
        return normalized;
    };
    let Some(hex) = literal.strip_suffix('\'') else {
        return normalized;
    };
    if !hex.len().is_multiple_of(2) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return normalized;
    }
    let value = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            u8::from_str_radix(
                std::str::from_utf8(pair).expect("hexadecimal bytes are ASCII"),
                16,
            )
            .expect("hexadecimal bytes decode")
        })
        .collect::<Vec<_>>();
    if upgrades::matches(table, name, &value) {
        definition.trim_end().to_owned()
    } else {
        normalized
    }
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

pub(super) fn validate_backup(connection: &Connection, node: NodeId) -> Result<(), Error> {
    let path = Path::new("backup");
    if !ownership(connection, path)? {
        identity(connection, path, node)?;
        verify(connection, path, DDL)
    } else {
        Err(incompatible(path, "backup database is empty"))
    }
}
