use super::{
    DDL, column,
    definition::{Table, normalize},
    incompatible, objects, upgrades,
};
use crate::Error;
use rusqlite::Connection;
use std::path::Path;

pub(super) struct Plan {
    statements: Vec<String>,
    pub(super) instructions: bool,
}

impl Plan {
    pub(super) fn prepare(connection: &Connection, path: &Path) -> Result<Self, Error> {
        let reference = Connection::open_in_memory()?;
        reference.execute_batch(DDL)?;
        let expected = objects(&reference)?;
        let actual = objects(connection)?;
        let instructions = actual
            .get(&("table".into(), "turns".into()))
            .map(|sql| Table::parse(sql).map(|table| !table.columns.contains_key("instructions")))
            .transpose()?
            .unwrap_or(false);
        let mut statements = Vec::new();
        for ((kind, name), wanted) in &expected {
            if kind != "table" {
                continue;
            }
            let Some(stored) = actual.get(&(kind.clone(), name.clone())) else {
                statements.push(wanted.clone());
                continue;
            };
            let wanted = Table::parse(wanted)?;
            let stored = Table::parse(stored)?;
            if wanted.header != stored.header
                || wanted.constraints != stored.constraints
                || wanted.options != stored.options
            {
                return Err(incompatible(
                    path,
                    format!("table {name:?} does not match the current constraints"),
                ));
            }
            for (field, existing) in &stored.columns {
                let declaration = wanted.columns.get(field).ok_or_else(|| {
                    incompatible(
                        path,
                        format!("column {name}.{field} is not part of the current schema"),
                    )
                })?;
                if column(name, field, existing) != column(name, field, declaration) {
                    return Err(incompatible(
                        path,
                        format!("column {name}.{field} does not match the current definition"),
                    ));
                }
            }
            for (field, declaration) in &wanted.columns {
                if stored.columns.contains_key(field) {
                    continue;
                }
                let mut declaration = declaration.clone();
                if upgrades::column(name, field).is_none()
                    && normalize(&declaration).contains("NOT NULL")
                    && !normalize(&declaration).contains("DEFAULT")
                    && connection.query_row(
                        &format!("SELECT EXISTS(SELECT 1 FROM \"{name}\")"),
                        [],
                        |row| row.get::<_, bool>(0),
                    )?
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
                statements.push(format!("ALTER TABLE \"{name}\" ADD COLUMN {declaration}"));
            }
        }
        for ((kind, name), wanted) in &expected {
            if kind == "table" {
                continue;
            }
            match actual.get(&(kind.clone(), name.clone())) {
                None => statements.push(wanted.clone()),
                Some(stored) if normalize(stored) == normalize(wanted) => {}
                Some(_) => {
                    return Err(incompatible(
                        path,
                        format!("schema object {name:?} does not match the current definition"),
                    ));
                }
            }
        }
        Ok(Self {
            statements,
            instructions,
        })
    }

    pub(super) fn apply(&self, connection: &Connection) -> Result<(), Error> {
        for statement in &self.statements {
            connection.execute_batch(statement)?;
        }
        Ok(())
    }
}
