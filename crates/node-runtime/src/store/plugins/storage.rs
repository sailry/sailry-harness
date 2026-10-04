//! Generic plugin data shares Node admission, receipts and the plugin package identity.
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    Command, ErrorCode, Fault, Output, Request,
    plugin::storage::{self, Declaration, Entry, Page, Scope},
};
use serde_json::Value;

use crate::store::database::storage_error;
pub(in crate::store) mod conversation;
#[cfg(test)]
mod schema_tests;
mod search;

pub(super) fn execute(db: &Connection, request: &Request) -> Result<Output, Fault> {
    let name = &request
        .plugin
        .as_ref()
        .ok_or_else(|| Fault::new(ErrorCode::PermissionDenied, "plugin provenance is required"))?
        .package
        .name;
    let declaration = declaration(db, request)?;
    match &request.command {
        Command::ReadPluginConversationValue { .. }
        | Command::WritePluginConversationValue { .. }
        | Command::RemovePluginConversationValue { .. } => {
            conversation::execute(db, request, declaration.as_ref())
        }
        Command::ReadPluginValue { key } => {
            check_key(key)?;
            let mut entry = read(db, name, key)?;
            project_defaults(
                declaration.as_ref(),
                Scope::Node,
                key,
                entry.present,
                &mut entry.value,
            )?;
            Ok(Output::PluginValue(entry))
        }
        Command::ListPluginKeys {
            prefix,
            after,
            limit,
        } => {
            check_prefix(prefix)?;
            if after
                .as_ref()
                .is_some_and(|after| !valid_key(after) || !after.starts_with(prefix))
                || !(1..=storage::MAX_PAGE_LIMIT).contains(limit)
            {
                return Err(invalid("invalid plugin key page"));
            }
            Ok(Output::PluginKeys(list(
                db,
                name,
                prefix,
                after.as_deref(),
                *limit,
            )?))
        }
        Command::WritePluginValue {
            key,
            value,
            expected_revision,
        } => {
            check_key(key)?;
            let value = normalize(
                db,
                declaration.as_ref(),
                name,
                key,
                value,
                *expected_revision,
            )?;
            let body = serde_json::to_vec(&value).map_err(storage_error)?;
            if body.len() > storage::MAX_VALUE_BYTES {
                return Err(invalid("plugin value exceeds 256 KiB"));
            }
            Ok(Output::PluginValue(write(
                db,
                name,
                key,
                &value,
                &body,
                *expected_revision,
                None,
            )?))
        }
        Command::WriteIndexedPluginValue {
            key,
            value,
            index,
            expected_revision,
        } => {
            check_key(key)?;
            let value = normalize(
                db,
                declaration.as_ref(),
                name,
                key,
                value,
                *expected_revision,
            )?;
            let body = serde_json::to_vec(&value).map_err(storage_error)?;
            if body.len() > storage::MAX_VALUE_BYTES {
                return Err(invalid("plugin value exceeds 256 KiB"));
            }
            search::validate_index(index)?;
            Ok(Output::PluginValue(write(
                db,
                name,
                key,
                &value,
                &body,
                *expected_revision,
                Some(index),
            )?))
        }
        Command::SearchPluginValues(query) => {
            let mut page = search::read(db, name, query)?;
            for mut entry in std::mem::take(&mut page.entries) {
                project_defaults(
                    declaration.as_ref(),
                    Scope::Node,
                    &entry.key,
                    entry.present,
                    &mut entry.value,
                )?;
                page.entries.push(entry);
                if serde_json::to_vec(&Output::PluginSearch(page.clone()))
                    .map_err(storage_error)?
                    .len()
                    + page.entries.len() * 2
                    + 8
                    > storage::MAX_SEARCH_BYTES
                {
                    page.entries.pop();
                    page.next = Some(query.offset + page.entries.len() as u16);
                    break;
                }
            }
            Ok(Output::PluginSearch(page))
        }
        Command::RemovePluginValue {
            key,
            expected_revision,
        } => {
            check_key(key)?;
            Ok(Output::PluginValue(remove(
                db,
                name,
                key,
                *expected_revision,
            )?))
        }
        _ => Err(Fault::new(
            ErrorCode::Internal,
            "plugin storage command expected",
        )),
    }
}

fn declaration(db: &Connection, request: &Request) -> Result<Option<Declaration>, Fault> {
    let context = request
        .plugin
        .as_ref()
        .ok_or_else(|| Fault::new(ErrorCode::PermissionDenied, "plugin provenance is required"))?;
    // Admission has already checked authority. Resolve code by its immutable reference,
    // including callbacks and turns that retain an older installed package revision.
    Ok(
        crate::store::agent::plugins::packages(db, std::slice::from_ref(&context.package))?
            .pop()
            .and_then(|package| package.extension)
            .and_then(|extension| extension.storage),
    )
}

fn schema<'a>(declaration: Option<&'a Declaration>, scope: Scope, key: &str) -> Option<&'a Value> {
    declaration?
        .collections
        .iter()
        .find(|collection| collection.matches(scope, key))
        .map(|collection| &collection.schema)
}

fn project_defaults(
    declaration: Option<&Declaration>,
    scope: Scope,
    key: &str,
    present: bool,
    value: &mut Value,
) -> Result<(), Fault> {
    if present && let Some(schema) = schema(declaration, scope, key) {
        crate::plugins::storage::project(schema, value)?;
    }
    Ok(())
}

fn normalize(
    db: &Connection,
    declaration: Option<&Declaration>,
    name: &str,
    key: &str,
    value: &Value,
    expected: u64,
) -> Result<Value, Fault> {
    let Some(schema) = schema(declaration, Scope::Node, key) else {
        return Ok(value.clone());
    };
    let current = read(db, name, key)?;
    check_revision(current.revision, expected)?;
    crate::plugins::storage::normalize(schema, current.present.then_some(&current.value), value)
}

fn valid_key(key: &str) -> bool {
    (1..=storage::MAX_KEY_BYTES).contains(&key.len()) && !key.contains('\0')
}

fn check_key(key: &str) -> Result<(), Fault> {
    if valid_key(key) {
        Ok(())
    } else {
        Err(invalid("invalid plugin value key"))
    }
}

fn check_prefix(prefix: &str) -> Result<(), Fault> {
    if prefix.len() <= storage::MAX_KEY_BYTES && !prefix.contains('\0') {
        Ok(())
    } else {
        Err(invalid("invalid plugin key prefix"))
    }
}

type StoredValue = (u64, Option<Vec<u8>>);

fn current(db: &Connection, name: &str, key: &str) -> Result<Option<StoredValue>, Fault> {
    let row: Option<(i64, Option<Vec<u8>>)> = db
        .query_row(
            "SELECT revision,value FROM plugin_values WHERE name=?1 AND key=?2",
            params![name, key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    row.map(|(revision, value)| {
        u64::try_from(revision)
            .ok()
            .filter(|revision| *revision > 0)
            .map(|revision| (revision, value))
            .ok_or_else(|| storage_error("invalid plugin value revision"))
    })
    .transpose()
}

pub(in crate::store) fn read(db: &Connection, name: &str, key: &str) -> Result<Entry, Fault> {
    let (revision, body) = current(db, name, key)?.unwrap_or((0, None));
    let value = body
        .as_deref()
        .map(serde_json::from_slice::<Value>)
        .transpose()
        .map_err(storage_error)?;
    Ok(Entry {
        key: key.into(),
        revision,
        present: body.is_some(),
        value: value.unwrap_or(Value::Null),
    })
}

fn list(
    db: &Connection,
    name: &str,
    prefix: &str,
    after: Option<&str>,
    limit: u16,
) -> Result<Page, Fault> {
    let mut query = db
        .prepare("SELECT key FROM plugin_values WHERE name=?1 AND value IS NOT NULL ORDER BY key")
        .map_err(storage_error)?;
    let mut keys = Vec::new();
    for key in query
        .query_map([name], |row| row.get::<_, String>(0))
        .map_err(storage_error)?
    {
        let key = key.map_err(storage_error)?;
        if key.starts_with(prefix) && after.is_none_or(|after| key.as_str() > after) {
            keys.push(key);
            if keys.len() > usize::from(limit) {
                break;
            }
        }
    }
    let more = keys.len() > usize::from(limit);
    keys.truncate(usize::from(limit));
    let after = more.then(|| keys.last().expect("nonempty page limit").clone());
    Ok(Page { keys, after })
}

fn check_revision(actual: u64, expected: u64) -> Result<(), Fault> {
    if actual == expected {
        Ok(())
    } else {
        Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin value changed",
        ))
    }
}

fn next_revision(revision: u64) -> Result<i64, Fault> {
    revision
        .checked_add(1)
        .and_then(|revision| i64::try_from(revision).ok())
        .ok_or_else(|| invalid("plugin value revision exhausted"))
}

fn write(
    db: &Connection,
    name: &str,
    key: &str,
    value: &Value,
    body: &[u8],
    expected: u64,
    index: Option<&storage::Index>,
) -> Result<Entry, Fault> {
    let existing = current(db, name, key)?;
    let revision = existing.as_ref().map_or(0, |(revision, _)| *revision);
    check_revision(revision, expected)?;
    let next = next_revision(revision)?;
    let indexed = index
        .map(serde_json::to_vec)
        .transpose()
        .map_err(storage_error)?;
    search::check(db, name)?;
    let (count, bytes): (i64, i64) = db
        .query_row(
            "SELECT count(*),coalesce(sum(length(value)+coalesce(length(index_data),0)),0) FROM plugin_values WHERE name=?1 AND value IS NOT NULL",
            [name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(storage_error)?;
    let old_body = existing
        .as_ref()
        .and_then(|(_, body)| body.as_ref())
        .map_or(0, Vec::len);
    let old_index: i64 = db.query_row("SELECT coalesce(sum(length(index_data)),0) FROM plugin_values WHERE name=?1 AND key=?2", params![name,key], |row|row.get(0)).map_err(storage_error)?;
    let old = old_body
        .checked_add(usize::try_from(old_index).map_err(storage_error)?)
        .ok_or_else(|| storage_error("invalid plugin storage size"))?;
    let total = usize::try_from(bytes)
        .ok()
        .and_then(|bytes| bytes.checked_sub(old))
        .and_then(|bytes| bytes.checked_add(body.len()))
        .and_then(|bytes| bytes.checked_add(indexed.as_ref().map_or(0, Vec::len)))
        .ok_or_else(|| storage_error("invalid plugin storage size"))?;
    if count + i64::from(existing.as_ref().is_none_or(|(_, body)| body.is_none()))
        > storage::MAX_NAMESPACE_KEYS as i64
        || total > storage::MAX_NAMESPACE_BYTES
    {
        return Err(Fault::new(ErrorCode::Busy, "plugin storage is full"));
    }
    db.execute(
        "INSERT INTO plugin_values(name,key,revision,value,index_data) VALUES(?1,?2,?3,?4,?5) \
         ON CONFLICT(name,key) DO UPDATE SET revision=excluded.revision,value=excluded.value,index_data=excluded.index_data",
        params![name, key, next, body, indexed],
    )
    .map_err(storage_error)?;
    search::replace(db, name, key, index)?;
    Ok(Entry {
        key: key.into(),
        revision: next as u64,
        value: value.clone(),
        present: true,
    })
}

fn remove(db: &Connection, name: &str, key: &str, expected: u64) -> Result<Entry, Fault> {
    let existing = current(db, name, key)?;
    let revision = existing.as_ref().map_or(0, |(revision, _)| *revision);
    check_revision(revision, expected)?;
    if existing.is_none() {
        return Ok(Entry {
            key: key.into(),
            revision: 0,
            value: Value::Null,
            present: false,
        });
    }
    let next = next_revision(revision)?;
    search::check(db, name)?;
    db.execute(
        "UPDATE plugin_values SET revision=?3,value=NULL,index_data=NULL WHERE name=?1 AND key=?2",
        params![name, key, next],
    )
    .map_err(storage_error)?;
    search::replace(db, name, key, None)?;
    Ok(Entry {
        key: key.into(),
        revision: next as u64,
        value: Value::Null,
        present: false,
    })
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE plugin_values(name TEXT, key TEXT, revision INTEGER, value BLOB, index_data BLOB, PRIMARY KEY(name,key));").unwrap();
        db
    }

    #[test]
    fn deletion_preserves_revisions() {
        let db = database();
        for index in 0..storage::MAX_NAMESPACE_KEYS {
            write(
                &db,
                "game",
                &index.to_string(),
                &Value::Null,
                b"null",
                0,
                None,
            )
            .unwrap();
        }
        assert_eq!(
            write(&db, "game", "extra", &Value::Null, b"null", 0, None)
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        let removed = remove(&db, "game", "0", 1).unwrap();
        assert!(!removed.present);
        write(&db, "game", "extra", &Value::Null, b"null", 0, None).unwrap();
        assert_eq!(
            write(
                &db,
                "game",
                "0",
                &Value::Null,
                b"null",
                removed.revision,
                None
            )
            .unwrap_err()
            .code,
            ErrorCode::Busy
        );
        remove(&db, "game", "extra", 1).unwrap();
        assert_eq!(
            write(
                &db,
                "game",
                "0",
                &Value::Null,
                b"null",
                removed.revision,
                None
            )
            .unwrap()
            .revision,
            3
        );
        assert_eq!(
            write(&db, "game", "0", &Value::Null, b"null", 1, None)
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }

    #[test]
    fn replacement_counts_only_current_value_bytes() {
        let db = database();
        let body = vec![b' '; storage::MAX_VALUE_BYTES];
        for index in 0..storage::MAX_NAMESPACE_BYTES / storage::MAX_VALUE_BYTES {
            write(
                &db,
                "game",
                &index.to_string(),
                &Value::Null,
                &body,
                0,
                None,
            )
            .unwrap();
        }
        assert_eq!(
            write(&db, "game", "extra", &Value::Null, b"null", 0, None)
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        write(&db, "game", "0", &Value::Null, b"null", 1, None).unwrap();
        write(&db, "game", "extra", &Value::Null, b"null", 0, None).unwrap();
        assert_eq!(read(&db, "game", "0").unwrap().revision, 2);
    }
}
