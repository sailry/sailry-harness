//! Captured plugin values provide raw sources for rebuildable semantic indexes.
use super::*;
use storage::{Collection, Index, Source};

#[cfg(test)]
mod tests;

pub(in crate::store) struct Namespace {
    name: String,
    collections: Vec<Collection>,
}

impl Namespace {
    pub(in crate::store) fn from_reference(
        db: &Connection,
        reference: &sailry_protocol::plugin::Reference,
    ) -> Result<Self, Fault> {
        let package = crate::store::agent::plugins::packages(db, std::slice::from_ref(reference))?
            .pop()
            .ok_or_else(|| {
                Fault::new(ErrorCode::NotConfigured, "captured plugin is unavailable")
            })?;
        let collections = package
            .extension
            .and_then(|extension| extension.storage)
            .map(|declaration| declaration.collections)
            .unwrap_or_default()
            .into_iter()
            .filter(|collection| {
                collection.scope == Scope::Node
                    && collection
                        .index
                        .as_ref()
                        .is_some_and(|index| index.semantic)
            })
            .collect();
        Ok(Self {
            name: reference.name.clone(),
            collections,
        })
    }

    fn contains(&self, key: &str) -> bool {
        self.collections
            .iter()
            .any(|collection| collection.matches(Scope::Node, key))
    }
}

pub(crate) struct Record {
    pub entry: Entry,
    pub index: Index,
    pub title: String,
    pub body: String,
    pub source: Source,
    pub source_revision: u64,
    pub source_hash: [u8; 32],
}

pub(in crate::store) fn record(
    db: &Connection,
    namespace: &Namespace,
    key: &str,
) -> Result<Option<Record>, Fault> {
    check_key(key)?;
    if !namespace.contains(key) {
        return Ok(None);
    }
    let entry = read(db, &namespace.name, key)?;
    if !entry.present {
        return Ok(None);
    }
    let index: Option<Vec<u8>> = db
        .query_row(
            "SELECT index_data FROM plugin_values WHERE name=?1 AND key=?2",
            params![namespace.name, key],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let Some(index) = index else {
        return Ok(None);
    };
    if index.len() > storage::MAX_INDEX_BYTES {
        return Err(Fault::invalid("semantic index exceeds the size limit"));
    }
    let index: Index = serde_json::from_slice(&index).map_err(storage_error)?;
    search::validate_index(&index)?;
    let Some(source) = index.source.clone() else {
        return Ok(None);
    };
    let captured = read(db, &namespace.name, &source.key)?;
    if !captured.present {
        return Ok(None);
    }
    let body = captured
        .value
        .pointer(&source.pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| Fault::invalid("semantic source must point to raw text"))?;
    if body.len() > storage::MAX_VALUE_BYTES || body.contains('\0') {
        return Err(Fault::invalid("semantic source text is invalid"));
    }
    Ok(Some(Record {
        title: index.fields[0].clone(),
        source_hash: *blake3::hash(body.as_bytes()).as_bytes(),
        body: body.into(),
        source_revision: captured.revision,
        entry,
        index,
        source,
    }))
}
