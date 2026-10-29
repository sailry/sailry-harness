//! Synchronize the current native schema without importing another database format.
pub(in crate::store) mod backup;
mod definition;
mod generation;
mod plan;
#[cfg(test)]
mod tests;
mod upgrades;

use crate::Error;
use definition::{Table, normalize};
use sailry_protocol::NodeId;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use turso::Connection;

type Objects = BTreeMap<(String, String), String>;
const DDL: &str = include_str!("../schema.sql");

// Retain these two inactive tables in existing profiles without creating or reading them.
const RETAINED: [(&str, &str); 2] = [
    (
        "code_settings",
        "CREATE TABLE code_settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL)",
    ),
    (
        "code_startups",
        "CREATE TABLE code_startups (
    worktree TEXT NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
    root TEXT NOT NULL,
    server TEXT NOT NULL,
    body BLOB NOT NULL,
    PRIMARY KEY(worktree,root,server)
)",
    ),
];

fn retained(actual: &mut Objects, path: &Path) -> Result<(), Error> {
    for (name, definition) in RETAINED {
        if let Some(stored) = actual.remove(&("table".into(), name.into()))
            && normalize(&stored) != normalize(definition)
        {
            return Err(incompatible(
                path,
                format!("retained table {name:?} does not match its definition"),
            ));
        }
    }
    Ok(())
}

pub(super) struct Schema {
    state: State,
    expected: Objects,
    path: PathBuf,
}

enum State {
    Fresh,
    Current,
    Update(plan::Plan),
    Rebuild,
}

impl Schema {
    pub(super) fn needs_update(&self) -> bool {
        matches!(self.state, State::Update(_))
    }

    pub(super) fn needs_rebuild(&self) -> bool {
        matches!(self.state, State::Rebuild)
    }

    /// The bootstrap owner supplies the transaction and owns rehearsal and backup.
    pub(super) async fn initialize(&self, connection: &Connection) -> Result<(), Error> {
        match &self.state {
            State::Fresh => connection.execute_batch(DDL).await?,
            State::Update(plan) => {
                plan.apply(connection).await?;
                upgrades::apply(connection).await?;
            }
            State::Current => {}
            State::Rebuild => {
                return Err(Error::Worker(
                    "Node storage without a generation requires a development reset".into(),
                ));
            }
        }
        if matches!(&self.state, State::Update(plan) if plan.adds("database_profiles", "password_target"))
        {
            crate::store::databases::credentials::reconcile(connection)
                .await
                .map_err(|error| Error::Worker(error.message))?;
        }
        if !matches!(self.state, State::Current) {
            if matches!(self.state, State::Fresh)
                || matches!(&self.state, State::Update(plan) if plan.creates("roles"))
            {
                crate::store::roles::initialize(connection).await?;
            }
            if matches!(self.state, State::Fresh)
                || matches!(&self.state, State::Update(plan) if plan.creates("local_model_settings"))
            {
                crate::store::local_models::initialize(connection)
                    .await
                    .map_err(|error| Error::Worker(error.message))?;
            }
            if matches!(self.state, State::Update(_)) {
                crate::store::plugins::storage::search::rebuild(connection)
                    .await
                    .map_err(|error| Error::Worker(error.message))?;
            }
            verify_objects(connection, &self.path, &self.expected).await?;
        }
        crate::store::providers::presets::discard_invalid_cache(connection)
            .await
            .map_err(|_| Error::Worker("channel catalog cache could not be discarded".into()))?;
        Ok(())
    }

    pub(super) async fn plugins(
        &self,
        connection: &Connection,
        plugins: Option<&crate::plugins::Host>,
    ) -> Result<(), Error> {
        if let Some(host) = plugins {
            crate::store::plugins::distribution::initialize(connection, host).await?;
        }
        if matches!(self.state, State::Current) {
            super::validation::startup(connection).await?;
        } else {
            super::validation::stored(connection).await?;
        }
        references(connection).await?;
        Ok(())
    }
}

/// Inspect only; the bootstrap owner decides when all handles can be closed for copying.
pub(super) async fn prepare(
    connection: &Connection,
    path: &Path,
    node: NodeId,
    plugins: Option<&crate::plugins::Host>,
) -> Result<Schema, Error> {
    let fresh = ownership(connection, path).await?;
    if !fresh {
        identity(connection, path, node).await?;
        if !generation::current(connection, path).await? {
            return Ok(Schema {
                state: State::Rebuild,
                expected: Objects::new(),
                path: path.to_owned(),
            });
        }
    }
    let values = if fresh {
        None
    } else {
        objects(connection)
            .await?
            .remove(&("table".into(), "plugin_values".into()))
    };
    let definitions = if let Some(values) = values {
        let has_index_data = Table::parse(&values)?.columns.contains_key("index_data");
        crate::store::plugins::storage::search::definitions(connection, has_index_data)
            .await
            .map_err(|_| incompatible(path, "plugin search declarations cannot be read"))?
    } else {
        Vec::new()
    };
    let expected = expected(DDL, &definitions).await?;
    let state = if fresh {
        State::Fresh
    } else {
        let plan = plan::Plan::prepare(connection, path, &expected, &definitions).await?;
        if plan.is_empty() {
            verify_objects(connection, path, &expected).await?;
            let defaults_changed = upgrades::needed(connection).await?;
            let connections_changed = if let Some(host) = plugins {
                crate::store::providers::authentication::needs_binding(connection).await?
                    || crate::store::plugins::distribution::needs_connections(connection, host)
                        .await?
            } else {
                false
            };
            if !defaults_changed
                && !connections_changed
                && crate::store::plugins::storage::search::validate(connection)
                    .await
                    .is_ok()
            {
                State::Current
            } else {
                State::Update(plan)
            }
        } else {
            State::Update(plan)
        }
    };
    Ok(Schema {
        state,
        expected,
        path: path.to_owned(),
    })
}

async fn identity(connection: &Connection, path: &Path, node: NodeId) -> Result<(), Error> {
    let stored = match connection
        .prepare("SELECT identity FROM node WHERE singleton=1")
        .await?
        .query_row(())
        .await
    {
        Ok(row) => Some(row.get::<Vec<u8>>(0)?),
        Err(turso::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(error.into()),
    };
    if stored.as_deref() != Some(node.0.as_slice()) {
        return Err(incompatible(
            path,
            "Node identity is missing or does not match its database",
        ));
    }
    Ok(())
}

async fn ownership(connection: &Connection, path: &Path) -> Result<bool, Error> {
    let application = connection
        .prepare("PRAGMA application_id")
        .await?
        .query_row(())
        .await?
        .get::<i64>(0)?;
    let version = connection
        .prepare("PRAGMA user_version")
        .await?
        .query_row(())
        .await?
        .get::<i64>(0)?;
    if application == 0 && version == 0 && objects(connection).await?.is_empty() {
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

/// This disposable reference has no user records and never shares the live profile.
async fn expected(ddl: &str, definitions: &[String]) -> Result<Objects, Error> {
    let mut temporary = tempfile::Builder::new();
    temporary.prefix(".schema-reference-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = temporary.tempdir()?;
    crate::profile::private_directory(directory.path())?;
    let database = super::engine(&directory.path().join("reference.turso")).await?;
    let connection = super::connect(&database).await?;
    connection.execute("BEGIN", ()).await?;
    connection.execute_batch(ddl).await?;
    for definition in definitions {
        connection.execute_batch(definition).await?;
    }
    connection.execute("COMMIT", ()).await?;
    let mut expected = objects(&connection).await?;
    let declarations = expected.clone();
    check_backing(&mut expected, &declarations, Path::new("reference"))?;
    Ok(expected)
}

async fn verify_objects(
    connection: &Connection,
    path: &Path,
    expected: &Objects,
) -> Result<(), Error> {
    let mut actual = objects(connection).await?;
    check_backing(&mut actual, expected, path)?;
    retained(&mut actual, path)?;
    compare(expected, &actual, path)?;
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

/// Native FTS backing objects are lazy and belong only to an exact declared index.
fn check_backing(actual: &mut Objects, expected: &Objects, path: &Path) -> Result<(), Error> {
    for ((kind, name), sql) in expected {
        if kind != "index" || !normalize(sql).contains("USING FTS(") {
            continue;
        }
        let table = format!("__turso_internal_fts_dir_{name}");
        let index = format!("{table}_key");
        let table_sql = format!(
            "CREATE TABLE {table} (path TEXT NOT NULL, chunk_no INTEGER NOT NULL, bytes BLOB NOT NULL)"
        );
        let index_sql = format!(
            "CREATE INDEX IF NOT EXISTS {index} ON {table} USING backing_btree (path, chunk_no, bytes)"
        );
        let table_object = actual.remove(&("table".into(), table));
        let index_object = actual.remove(&("index".into(), index));
        match (table_object, index_object) {
            (None, None) => {}
            (Some(stored_table), Some(stored_index))
                if normalize(&stored_table) == normalize(&table_sql)
                    && normalize(&stored_index) == normalize(&index_sql) => {}
            _ => {
                return Err(incompatible(
                    path,
                    "native search backing objects do not match their declared index",
                ));
            }
        }
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

async fn objects(connection: &Connection) -> Result<Objects, Error> {
    let mut rows = connection.query(
        "SELECT name,type,sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' AND sql IS NOT NULL", ()
    ).await?;
    let mut result = BTreeMap::new();
    while let Some(row) = rows.next().await? {
        let name = row.get::<String>(0)?;
        let kind = row.get::<String>(1)?;
        let sql = row.get::<String>(2)?;
        // Turso 0.8.3 persists its checkpoint watermark in this one fixed engine table.
        if name == "__turso_internal_mvcc_meta" && kind == "table" {
            let stored = Table::parse(&sql)?;
            let expected =
                Table::parse("CREATE TABLE __turso_internal_mvcc_meta(k TEXT,v INTEGER NOT NULL)")?;
            if stored.header != expected.header
                || stored.columns != expected.columns
                || stored.constraints != expected.constraints
                || stored.options != expected.options
            {
                return Err(Error::Worker(
                    "invalid native database metadata; existing data was not changed".into(),
                ));
            }
            continue;
        }
        result.insert((kind, name), sql);
    }
    Ok(result)
}

fn incompatible(path: &Path, reason: impl std::fmt::Display) -> Error {
    Error::Worker(format!(
        "incompatible development database at {path:?}: {reason}; existing data was not changed; use --data-dir <new absolute private directory> to start with a fresh profile"
    ))
}

pub(super) async fn validate_backup(connection: &Connection, node: NodeId) -> Result<(), Error> {
    let path = Path::new("backup");
    if ownership(connection, path).await? {
        return Err(incompatible(path, "backup database is empty"));
    }
    identity(connection, path, node).await?;
    let definitions = crate::store::plugins::storage::search::definitions(connection, true)
        .await
        .map_err(|_| incompatible(path, "plugin search declarations cannot be read"))?;
    let expected = expected(DDL, &definitions).await?;
    verify_objects(connection, path, &expected).await?;
    references(connection).await
}

/// Audit the current owned relationships without SQLite's unavailable check pragma.
pub(in crate::store) async fn references(connection: &Connection) -> Result<(), Error> {
    let tables = definition::table_names(DDL)?;
    for table in tables {
        audit_references(connection, &table).await?;
    }
    Ok(())
}

async fn audit_references(connection: &Connection, table: &str) -> Result<(), Error> {
    let mut rows = connection
        .query(format!("PRAGMA foreign_key_list({})", quote(table)), ())
        .await?;
    let mut keys = BTreeMap::<i64, (String, BTreeMap<i64, (String, String)>)>::new();
    while let Some(row) = rows.next().await? {
        let id = row.get::<i64>(0)?;
        let position = row.get::<i64>(1)?;
        let parent = row.get::<String>(2)?;
        let child_column = row.get::<String>(3)?;
        let parent_column = row.get::<String>(4)?;
        let key = keys
            .entry(id)
            .or_insert_with(|| (parent.clone(), BTreeMap::new()));
        if key.0 != parent
            || key
                .1
                .insert(position, (child_column, parent_column))
                .is_some()
        {
            return Err(Error::Worker(
                "invalid stored foreign key declaration".into(),
            ));
        }
    }
    drop(rows);
    for (_, (parent, columns)) in keys {
        let present = columns
            .values()
            .map(|(child, _)| format!("c.{} IS NOT NULL", quote(child)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let matching = columns
            .values()
            .map(|(child, parent)| format!("p.{}=c.{}", quote(parent), quote(child)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let row = connection.prepare(format!(
            "SELECT EXISTS(SELECT 1 FROM {} c WHERE {present} AND NOT EXISTS(SELECT 1 FROM {} p WHERE {matching}))",
            quote(table), quote(&parent)
        )).await?.query_row(()).await?;
        if row.get::<bool>(0)? {
            return Err(Error::Worker(format!(
                "invalid stored reference in {table}; existing data was not changed"
            )));
        }
    }
    Ok(())
}

fn quote(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}
