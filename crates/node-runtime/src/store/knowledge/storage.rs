//! Explicit collections read raw values from their durable captured namespace.
use super::{Stored, collections};
use crate::store::database::storage_error;
use rusqlite::{Connection, OptionalExtension, params};
use sailry_link::CancellationToken;
use sailry_protocol::{
    ErrorCode, Fault,
    knowledge::Source,
    plugin::storage::{self, Scope},
};
use serde_json::Value;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
pub(crate) struct Record {
    pub(crate) key: String,
    pub(crate) revision: u64,
    pub(crate) hash: [u8; 32],
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) source: storage::Source,
    pub(crate) source_revision: u64,
    pub(crate) source_hash: [u8; 32],
    pub(crate) tags: Vec<String>,
}

pub(crate) struct PageRaw {
    pub(crate) records: Vec<Record>,
    pub(crate) after: Option<String>,
}

struct Raw {
    revision: u64,
    body: Vec<u8>,
    index: Option<Vec<u8>>,
}

pub(in crate::store) fn page(
    db: &Connection,
    stored: &Stored,
    after: Option<&str>,
    limit: u16,
    stop: &CancellationToken,
) -> Result<PageRaw, Fault> {
    check(stop)?;
    if !(1..=16).contains(&limit) || after.is_some_and(|key| !valid_key(key)) {
        return Err(Fault::invalid("invalid knowledge source page"));
    }
    if !current(db, stored)? {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "knowledge collection changed",
        ));
    }
    if stored.automatic {
        return super::semantic::sources(db, stored, after, limit, stop);
    }
    let (name, scope, prefix) = namespace(stored)?;
    let mut statement = db.prepare(match scope {
        Scope::Node => "SELECT key FROM plugin_values WHERE name=?1 AND value IS NOT NULL AND substr(key,1,length(?2))=?2 AND (?3 IS NULL OR key>?3) ORDER BY key LIMIT ?4",
        Scope::Conversation => "SELECT v.key FROM plugin_conversation_values v JOIN plugin_conversation_records r ON r.sequence=v.record WHERE v.name=?1 AND r.value IS NOT NULL AND substr(v.key,1,length(?2))=?2 AND (?3 IS NULL OR v.key>?3) AND v.session=?5 ORDER BY v.key LIMIT ?4",
    }).map_err(storage_error)?;
    let session = stored.capture.session.map(|id| id.to_string());
    let mut rows = if scope == Scope::Node {
        statement.query(params![name, prefix, after, i64::from(limit) + 1])
    } else {
        statement.query(params![name, prefix, after, i64::from(limit) + 1, session])
    }
    .map_err(storage_error)?;
    let mut page = PageRaw {
        records: vec![],
        after: None,
    };
    let mut last = None;
    let mut scanned = 0;
    while let Some(row) = rows.next().map_err(storage_error)? {
        check(stop)?;
        if scanned == limit {
            page.after = last;
            break;
        }
        let key: String = row.get(0).map_err(storage_error)?;
        if let Some(record) = capture(db, stored, &key)? {
            page.records.push(record);
        }
        last = Some(key);
        scanned += 1;
    }
    Ok(page)
}

pub(in crate::store) fn unchanged(
    db: &Connection,
    stored: &Stored,
    record: &Record,
    stop: &CancellationToken,
) -> Result<bool, Fault> {
    check(stop)?;
    if !current(db, stored)? {
        return Ok(false);
    }
    if stored.automatic {
        return super::semantic::unchanged(db, stored, record, stop);
    }
    let (_, _, prefix) = namespace(stored)?;
    if !record.key.starts_with(prefix) {
        return Ok(false);
    }
    let Some(metadata) = read(db, stored, &record.key)? else {
        return Ok(false);
    };
    let Some(source) = read(db, stored, &record.source.key)? else {
        return Ok(false);
    };
    Ok(metadata.revision == record.revision
        && hash(&metadata) == record.hash
        && source.revision == record.source_revision
        && *blake3::hash(&source.body).as_bytes() == record.source_hash)
}

pub(in crate::store) fn record(
    db: &Connection,
    stored: &Stored,
    key: &str,
    stop: &CancellationToken,
) -> Result<Option<Record>, Fault> {
    check(stop)?;
    if !current(db, stored)? {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "knowledge collection changed",
        ));
    }
    if stored.automatic {
        return super::semantic::source(db, stored, key, stop);
    }
    let (_, _, prefix) = namespace(stored)?;
    if !valid_key(key) || !key.starts_with(prefix) {
        return Err(Fault::invalid("knowledge key is outside its source"));
    }
    capture(db, stored, key)
}

fn current(db: &Connection, stored: &Stored) -> Result<bool, Fault> {
    Ok(collections::load(db)?.into_iter().any(|current| {
        current.collection.id == stored.collection.id
            && current.collection.revision == stored.collection.revision
            && current.collection.rebuild == stored.collection.rebuild
            && current.collection.paused == stored.collection.paused
            && current.collection.progress.generation == stored.collection.progress.generation
            && current.collection.owner == stored.collection.owner
            && current.collection.definition == stored.collection.definition
            && current.capture.package == stored.capture.package
            && current.capture.worktree == stored.capture.worktree
            && current.capture.session == stored.capture.session
            && current.automatic == stored.automatic
    }))
}

fn namespace(stored: &Stored) -> Result<(&str, Scope, &str), Fault> {
    let Source::Storage { scope, prefix } = &stored.collection.definition.source else {
        return Err(Fault::invalid("knowledge storage source expected"));
    };
    let package = stored
        .capture
        .package
        .as_ref()
        .ok_or_else(|| Fault::invalid("knowledge storage owner is missing"))?;
    if *scope == Scope::Conversation && stored.capture.session.is_none() {
        return Err(Fault::invalid("knowledge storage session is missing"));
    }
    Ok((&package.name, *scope, prefix))
}

fn capture(db: &Connection, stored: &Stored, key: &str) -> Result<Option<Record>, Fault> {
    let Some(raw) = read(db, stored, key)? else {
        return Ok(None);
    };
    let value: Value = serde_json::from_slice(&raw.body).map_err(storage_error)?;
    let index: Option<storage::Index> = raw
        .index
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(storage_error)?;
    if let Some(index) = &index {
        crate::store::plugins::storage::search::validate_index(index)?;
    }
    let source = index
        .as_ref()
        .and_then(|index| index.source.clone())
        .unwrap_or_else(|| storage::Source {
            key: key.into(),
            pointer: String::new(),
        });
    let Some(original) = read(db, stored, &source.key)? else {
        return Ok(None);
    };
    let body = if index.as_ref().is_some_and(|index| index.source.is_some()) {
        let source_value: Value = serde_json::from_slice(&original.body).map_err(storage_error)?;
        source_value
            .pointer(&source.pointer)
            .and_then(Value::as_str)
            .ok_or_else(|| Fault::invalid("knowledge source pointer must select raw text"))?
            .to_owned()
    } else if let Some(text) = value.as_str() {
        text.to_owned()
    } else {
        String::from_utf8(raw.body.clone()).map_err(storage_error)?
    };
    if body.len() > storage::MAX_VALUE_BYTES || body.contains('\0') {
        return Err(Fault::invalid(
            "knowledge source text exceeds its size limit",
        ));
    }
    let title = value
        .get("title")
        .or_else(|| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_owned();
    Ok(Some(Record {
        key: key.into(),
        revision: raw.revision,
        hash: hash(&raw),
        title,
        body,
        source,
        source_revision: original.revision,
        source_hash: *blake3::hash(&original.body).as_bytes(),
        tags: index.map(|index| index.tags).unwrap_or_default(),
    }))
}

fn read(db: &Connection, stored: &Stored, key: &str) -> Result<Option<Raw>, Fault> {
    if !valid_key(key) {
        return Err(Fault::invalid("invalid knowledge storage key"));
    }
    let (name, scope, _) = namespace(stored)?;
    let row: Option<(i64, Vec<u8>, Option<Vec<u8>>)> = match scope {
        Scope::Node => db.query_row("SELECT revision,value,index_data FROM plugin_values WHERE name=?1 AND key=?2 AND value IS NOT NULL", params![name,key], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))),
        Scope::Conversation => db.query_row("SELECT v.revision,r.value,NULL FROM plugin_conversation_values v JOIN plugin_conversation_records r ON r.sequence=v.record WHERE v.name=?1 AND v.session=?2 AND v.key=?3 AND r.value IS NOT NULL", params![name,stored.capture.session.map(|id| id.to_string()),key], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))),
    }.optional().map_err(storage_error)?;
    row.map(|(revision, body, index)| {
        if body.len() > storage::MAX_VALUE_BYTES
            || index
                .as_ref()
                .is_some_and(|index| index.len() > storage::MAX_INDEX_BYTES)
        {
            return Err(Fault::invalid(
                "knowledge storage source exceeds its size limit",
            ));
        }
        Ok(Raw {
            revision: u64::try_from(revision).map_err(storage_error)?,
            body,
            index,
        })
    })
    .transpose()
}

fn hash(raw: &Raw) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(raw.body.len() as u64).to_le_bytes());
    hasher.update(&raw.body);
    if let Some(index) = &raw.index {
        hasher.update(index);
    }
    *hasher.finalize().as_bytes()
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= storage::MAX_KEY_BYTES && !key.contains('\0')
}

fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        Err(Fault::new(
            ErrorCode::Cancelled,
            "knowledge source read was cancelled",
        ))
    } else {
        Ok(())
    }
}
