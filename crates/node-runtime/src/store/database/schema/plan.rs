use super::{
    Objects, check_backing, column,
    definition::{Table, normalize},
    incompatible, objects, quote, upgrades,
};
use crate::Error;
use std::path::Path;
use turso::Connection;

#[derive(Debug)]
pub(super) struct Plan {
    statements: Vec<String>,
    created: std::collections::BTreeSet<String>,
    added: std::collections::BTreeSet<(String, String)>,
    refresh: bool,
}

impl Plan {
    pub(super) async fn prepare(
        connection: &Connection,
        path: &Path,
        expected: &Objects,
        definitions: &[String],
    ) -> Result<Self, Error> {
        let mut actual = objects(connection).await?;
        check_backing(&mut actual, expected, path)?;
        super::retained(&mut actual, path)?;
        let mut statements = Vec::new();
        let mut created = std::collections::BTreeSet::new();
        let mut added = std::collections::BTreeSet::new();
        let mut refresh = false;
        for ((kind, name), wanted) in expected {
            if kind != "table" {
                continue;
            }
            let Some(stored) = actual.get(&(kind.clone(), name.clone())) else {
                statements.push(wanted.clone());
                created.insert(name.clone());
                continue;
            };
            let wanted = Table::parse(wanted)?;
            let stored = Table::parse(stored)?;
            if let Some(field) = stored
                .columns
                .keys()
                .find(|field| !wanted.columns.contains_key(*field))
            {
                return Err(incompatible(
                    path,
                    format!("column {name}.{field} is not part of the current schema"),
                ));
            }
            for (field, declaration) in &wanted.columns {
                if let Some(existing) = stored.columns.get(field)
                    && column(name, field, existing) != column(name, field, declaration)
                {
                    return Err(incompatible(
                        path,
                        format!("column {name}.{field} does not match the current definition"),
                    ));
                }
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
            for (field, declaration) in &wanted.columns {
                if stored.columns.contains_key(field) {
                    continue;
                }
                let mut declaration = declaration.clone();
                if upgrades::column(name, field).is_none()
                    && normalize(&declaration).contains("NOT NULL")
                    && !normalize(&declaration).contains("DEFAULT")
                    && connection
                        .prepare(format!("SELECT EXISTS(SELECT 1 FROM {})", quote(name)))
                        .await?
                        .query_row(())
                        .await?
                        .get::<bool>(0)?
                {
                    return Err(incompatible(
                        path,
                        format!("column {name}.{field} requires a default for existing records"),
                    ));
                }
                if let Some(value) = upgrades::column(name, field)
                    && !normalize(&declaration).contains("DEFAULT")
                {
                    let value = value
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<String>();
                    declaration.push_str(&format!(" DEFAULT X'{value}'"));
                }
                statements.push(format!(
                    "ALTER TABLE {} ADD COLUMN {declaration}",
                    quote(name)
                ));
                added.insert((name.clone(), field.clone()));
            }
        }
        for ((kind, name), wanted) in expected {
            if kind == "table" {
                continue;
            }
            match actual.get(&(kind.clone(), name.clone())) {
                None => statements.push(wanted.clone()),
                Some(stored) if normalize(stored) == normalize(wanted) => {}
                Some(stored) if kind == "index" && upgrades::replaces_index(name, stored) => {
                    statements.push(format!("DROP INDEX {}", quote(name)));
                    statements.push(wanted.clone());
                }
                Some(stored) if kind == "index" && refreshable(stored, wanted, definitions) => {
                    // The search owner rebuilds its declared corpus in the same update transaction.
                    refresh = true;
                }
                Some(_) => {
                    return Err(incompatible(
                        path,
                        format!("schema object {name:?} does not match the current definition"),
                    ));
                }
            }
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
        Ok(Self {
            statements,
            created,
            added,
            refresh,
        })
    }

    pub(super) fn creates(&self, table: &str) -> bool {
        self.created.contains(table)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.statements.is_empty() && !self.refresh
    }

    pub(super) fn adds(&self, table: &str, field: &str) -> bool {
        self.added.contains(&(table.to_owned(), field.to_owned()))
    }

    pub(super) async fn apply(&self, connection: &Connection) -> Result<(), Error> {
        for statement in &self.statements {
            connection.execute_batch(statement).await?;
        }
        Ok(())
    }
}

fn refreshable(stored: &str, wanted: &str, definitions: &[String]) -> bool {
    let wanted = normalize(wanted);
    if !definitions.iter().any(|sql| normalize(sql) == wanted) {
        return false;
    }
    let stored = normalize(stored);
    matches!(
        (stored.split_once(" USING FTS("), wanted.split_once(" USING FTS(")),
        (Some((stored, _)), Some((wanted, _))) if stored == wanted
    )
}
