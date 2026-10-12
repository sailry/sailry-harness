//! Startup defaults for stored approvals and metadata for retired tools.
use crate::Error;
use rusqlite::{Connection, params};
use serde_json::Value;

#[cfg(test)]
mod tests;

/// The startup owner preserves a whole-database backup and validates in its transaction.
pub(super) fn reconcile(db: &Connection) -> Result<bool, Error> {
    let mut changed = false;
    for (table, column) in [
        ("agent_approvals", "body"),
        ("plugins", "body"),
        ("plugin_packages", "body"),
        ("requests", "result"),
        ("artifacts", "body"),
        ("artifact_revisions", "body"),
    ] {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )?;
        if !exists {
            continue;
        }
        let sql = format!(
            "SELECT rowid,CAST({column} AS BLOB) FROM {table} WHERE {column} IS NOT NULL ORDER BY rowid"
        );
        let records = db
            .prepare(&sql)?
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (row, before) in records {
            let Ok(mut value) = serde_json::from_slice::<Value>(&before) else {
                // The canonical validator reports syntax locations without payload values.
                continue;
            };
            let updated = match table {
                "agent_approvals" => approval(&mut value),
                "requests" => receipt(&mut value),
                "artifacts" | "artifact_revisions" => artifact(&mut value),
                _ => package(&mut value),
            };
            if !updated {
                continue;
            }
            let after = serde_json::to_string(&value).map_err(|_| {
                Error::Worker(
                    "startup defaults could not be encoded; existing data was not changed".into(),
                )
            })?;
            let sql = format!("UPDATE {table} SET {column}=?1 WHERE rowid=?2");
            if table == "requests" {
                db.execute(&sql, params![after, row])?;
            } else {
                db.execute(&sql, params![after.as_bytes(), row])?;
            }
            changed = true;
        }
    }
    Ok(changed)
}

fn artifact(value: &mut Value) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    if object.contains_key("kind") {
        return false;
    }
    object.insert("kind".into(), Value::String("document".into()));
    true
}

fn approval(value: &mut Value) -> bool {
    let Some(object) = value.as_object_mut() else {
        return false;
    };
    let mut changed = false;
    if !object.contains_key("can_remember") {
        object.insert("can_remember".into(), Value::Bool(false));
        changed = true;
    }
    if !object.contains_key("choice")
        && object.get("state").and_then(Value::as_str) == Some("approved")
        && object.get("source").and_then(Value::as_str) == Some("user")
    {
        object.insert("choice".into(), Value::String("once".into()));
        changed = true;
    }
    changed
}

fn package(value: &mut Value) -> bool {
    let Some(tools) = value
        .get_mut("extension")
        .and_then(|extension| extension.get_mut("tools"))
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    let mut issues = Vec::new();
    for (index, declaration) in tools.iter_mut().enumerate() {
        let direct = declaration.get("operation").and_then(Value::as_str) == Some("office.export");
        let prepared = declaration
            .get("handler")
            .and_then(|handler| handler.get("operation"))
            .and_then(Value::as_str)
            == Some("office.export");
        if direct || prepared {
            declaration["operation"] = Value::Null;
            declaration["handler"] = Value::Null;
            let field = if direct {
                "operation"
            } else {
                "handler.operation"
            };
            issues.push(serde_json::json!({
                "path": format!("plugin.json.extensions.dev.sailry.platform.tools[{index}].{field}"),
                "kind": "invalid_extensions"
            }));
        }
    }
    if issues.is_empty() {
        return false;
    }
    let recorded = value
        .as_object_mut()
        .unwrap()
        .entry("issues")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(recorded) = recorded.as_array_mut() {
        for issue in issues {
            if !recorded.contains(&issue) {
                recorded.push(issue);
            }
        }
    }
    true
}

fn receipt(value: &mut Value) -> bool {
    let Some(output) = value.get_mut("Ok") else {
        return false;
    };
    match output.get("kind").and_then(Value::as_str) {
        Some("approval") => output.get_mut("data").is_some_and(approval),
        Some("plugin") => output.get_mut("data").is_some_and(package),
        _ => false,
    }
}
