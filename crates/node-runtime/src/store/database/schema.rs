//! Synchronize safe additions from the single current DDL, without migration versions.
mod definition;
#[cfg(test)]
mod tests;

use crate::Error;
use definition::{Table, normalize};
use rusqlite::{Connection, MAIN_DB, OptionalExtension};
use sailry_protocol::NodeId;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const DDL: &str = include_str!("../schema.sql");

pub(super) struct Plan {
    fresh: bool,
    statements: Vec<String>,
}

impl Plan {
    pub(super) fn apply(&self, connection: &Connection) -> Result<(), Error> {
        for statement in &self.statements {
            connection.execute_batch(statement)?;
        }
        Ok(())
    }

    pub(super) fn backup(
        &self,
        connection: &Connection,
        path: &Path,
    ) -> Result<Option<PathBuf>, Error> {
        if self.fresh || self.statements.is_empty() {
            return Ok(None);
        }
        let directory = path
            .parent()
            .ok_or_else(|| incompatible(path, "database directory is missing"))?;
        // A private new file never overwrites an earlier snapshot. SQLite's backup
        // API includes committed WAL pages, unlike copying the main database file.
        let snapshot = tempfile::Builder::new()
            .prefix("schema-backup-")
            .suffix(".sqlite3")
            .tempfile_in(directory)?;
        connection.backup(MAIN_DB, snapshot.path(), None)?;
        snapshot.as_file().sync_all()?;
        let (_, path) = snapshot.keep().map_err(|error| error.error)?;
        Ok(Some(path))
    }
}

pub(super) fn prepare(connection: &Connection, path: &Path, node: NodeId) -> Result<Plan, Error> {
    let fresh = ownership(connection, path)?;
    if !fresh {
        for (table, column, detail) in [
            (
                "plugins",
                "settings_revision",
                "plugin configuration revision counter is missing",
            ),
            (
                "node",
                "plugin_defaults",
                "plugin initialization marker is missing",
            ),
        ] {
            let missing: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1) AND NOT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name=?2)",
                [table, column],
                |row| row.get(0),
            )?;
            if missing {
                // Historical configuration markers cannot be reconstructed from a default.
                return Err(incompatible(path, detail));
            }
        }
        let missing_state: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sessions') AND NOT EXISTS(SELECT 1 FROM pragma_table_info('sessions') WHERE name='state')",
            [],
            |row| row.get(0),
        )?;
        if missing_state
            && connection
                .prepare("SELECT 1 FROM sessions LIMIT 1")?
                .exists([])?
        {
            // A default cannot recover whether an existing session was archived or removed.
            return Err(incompatible(
                path,
                "session lifecycle state is missing for existing sessions",
            ));
        }
        let removed: Option<String> = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('credentials','plugin_credentials') LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(table) = removed {
            return Err(incompatible(
                path,
                format!("table {table:?} is not part of the current configuration storage"),
            ));
        }
    }
    let plan = if fresh {
        Plan {
            fresh,
            statements: vec![DDL.to_owned()],
        }
    } else {
        additions(connection, path, DDL)?
    };
    if fresh {
        return Ok(plan);
    }
    if plan.statements.is_empty() {
        validate(connection, path, node)?;
    } else {
        // Validate the complete proposal before changing the original's journal
        // or recovery state. Never manufacture historical values to pass checks.
        let mut preview = Connection::open_in_memory()?;
        {
            let backup = rusqlite::backup::Backup::new(connection, &mut preview)?;
            if backup.step(-1)? != rusqlite::backup::StepResult::Done {
                return Err(incompatible(
                    path,
                    "database could not be inspected consistently",
                ));
            }
        }
        plan.apply(&preview).map_err(|_| {
            incompatible(path, "schema additions violate stored data or constraints")
        })?;
        validate(&preview, path, node)?;
        integrity(&preview, path)?;
    }
    Ok(plan)
}

fn ownership(connection: &Connection, path: &Path) -> Result<bool, Error> {
    let application: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let objects: i64 = connection.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
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

fn validate(connection: &Connection, path: &Path, node: NodeId) -> Result<(), Error> {
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
    Ok(())
}

fn integrity(connection: &Connection, path: &Path) -> Result<(), Error> {
    let check: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if check != "ok" {
        return Err(incompatible(
            path,
            "stored data fails database integrity checks",
        ));
    }
    if connection.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return Err(incompatible(
            path,
            "stored data has missing relational references",
        ));
    }
    Ok(())
}

fn additions(connection: &Connection, path: &Path, ddl: &str) -> Result<Plan, Error> {
    let expected = Connection::open_in_memory()?;
    expected.execute_batch(ddl)?;
    let actual = objects(connection)?;
    let mut statements = vec![];
    for (name, (kind, sql)) in objects(&expected)? {
        let Some((actual_kind, actual_sql)) = actual.get(&name) else {
            statements.push(sql);
            continue;
        };
        if actual_kind != &kind {
            return Err(incompatible(
                path,
                format!("schema object {name:?} has a different kind"),
            ));
        }
        if kind == "index" {
            if normalize(actual_sql) != normalize(&sql) {
                statements.push(format!("DROP INDEX {}; {sql}", quote(&name)));
            }
            continue;
        }
        let wanted = Table::parse(&sql)?;
        let stored = Table::parse(actual_sql)?;
        if wanted.constraints != stored.constraints {
            return Err(incompatible(
                path,
                format!("table {name:?} requires an explicit constraint change"),
            ));
        }
        for (column, declaration) in &wanted.columns {
            if let Some(existing) = stored.columns.get(column) {
                if normalize(existing) != normalize(declaration) {
                    return Err(incompatible(
                        path,
                        format!("column {name}.{column} requires an explicit definition change"),
                    ));
                }
                continue;
            }
            let (not_null, default, primary, unique): (bool, Option<String>, i64, bool) =
                expected.query_row(
                    "SELECT \"notnull\",dflt_value,pk,EXISTS(SELECT 1 FROM pragma_index_list(?1) indices JOIN pragma_index_info(indices.name) fields WHERE indices.origin='u' AND fields.name=?2) FROM pragma_table_info(?1) WHERE name=?2",
                    [&name, column],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )?;
            if primary != 0 || unique || (not_null && !has_default(default.as_deref())) {
                return Err(incompatible(
                    path,
                    format!(
                        "column {name}.{column} cannot be added without an explicit data transformation"
                    ),
                ));
            }
            statements.push(format!(
                "ALTER TABLE {} ADD COLUMN {declaration}",
                quote(&name)
            ));
        }
        // Removed fields stay on disk. Required obsolete fields would prevent
        // current inserts, so report the conflict instead of dropping their data.
        for column in stored
            .columns
            .keys()
            .filter(|column| !wanted.columns.contains_key(*column))
        {
            let (not_null, default): (bool, Option<String>) = connection.query_row(
                "SELECT \"notnull\",dflt_value FROM pragma_table_info(?1) WHERE name=?2",
                [&name, column],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            if not_null && !has_default(default.as_deref()) {
                return Err(incompatible(
                    path,
                    format!(
                        "retained column {name}.{column} requires an explicit data transformation"
                    ),
                ));
            }
        }
    }
    // Tables and columns precede indexes. Unused indexes stay; changed
    // definitions are rebuilt in the same transaction.
    statements.sort_by_key(|sql| {
        let sql = normalize(sql);
        sql.starts_with("CREATE INDEX")
            || sql.starts_with("CREATE UNIQUE INDEX")
            || sql.starts_with("DROP INDEX")
    });
    Ok(Plan {
        fresh: false,
        statements,
    })
}

fn has_default(value: Option<&str>) -> bool {
    value.is_some_and(|value| normalize(value).trim_matches(['(', ')']) != "NULL")
}

fn objects(connection: &Connection) -> Result<BTreeMap<String, (String, String)>, Error> {
    let mut query = connection.prepare(
        "SELECT name,type,sql FROM sqlite_master WHERE type IN ('table','index') AND name NOT LIKE 'sqlite_%' AND sql IS NOT NULL",
    )?;
    Ok(query
        .query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?
        .collect::<Result<_, _>>()?)
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn incompatible(path: &Path, reason: impl std::fmt::Display) -> Error {
    Error::Worker(format!(
        "incompatible development database at {path:?}: {reason}; existing data was not changed; use --data-dir <new absolute private directory> to start with a fresh profile"
    ))
}
