//! Bounded FTS queries over each captured package's private corpus.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
use storage::{Index, Search, SearchPage};
mod indexes;
pub(in crate::store) use indexes::{apply as rebuild, validate};
#[cfg(test)]
mod tests;

fn table(name: &str) -> String {
    format!("plugin_search_{}", blake3::hash(name.as_bytes()).to_hex())
}

fn definition(name: &str) -> String {
    let table = table(name);
    format!(
        "CREATE VIRTUAL TABLE {table} USING fts5(field0,field1,cjk0,cjk1,key UNINDEXED,tags UNINDEXED,ordinal UNINDEXED,tokenize='unicode61 remove_diacritics 2 tokenchars ''+#''')"
    )
}

/// Reconstruct current schemas from durable corpus namespaces, including empty indexes.
/// The caller creates these only in a reference database, never in the stored profile.
pub(in crate::store) fn definitions(db: &Connection) -> Result<Vec<String>, Fault> {
    let mut definitions = Vec::new();
    for (name, indexed) in catalog(db)? {
        if indexed || exists(db, &table(&name))? {
            definitions.push(definition(&name));
        }
    }
    Ok(definitions)
}

fn catalog(db: &Connection) -> Result<Vec<(String, bool)>, Fault> {
    let mut query = db
        .prepare(
            "SELECT name, max(index_data IS NOT NULL) FROM plugin_values GROUP BY name ORDER BY name",
        )
        .map_err(storage_error)?;
    let names = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    Ok(names)
}

fn exists(db: &Connection, table: &str) -> Result<bool, Fault> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )
    .map_err(storage_error)
}

/// Reads never create or repair an index. Metadata is the durable ownership marker.
pub(super) fn check(db: &Connection, name: &str) -> Result<bool, Fault> {
    let table = table(name);
    let expected: i64 = db
        .query_row(
            "SELECT count(*) FROM plugin_values WHERE name=?1 AND index_data IS NOT NULL",
            [name],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if !exists(db, &table)? {
        return if expected == 0 {
            Ok(false)
        } else {
            Err(storage_error("plugin search index is missing"))
        };
    }
    let actual: i64 = db
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(storage_error)?;
    let matched: i64 = db.query_row(&format!("SELECT count(*) FROM {table} f JOIN plugin_values v ON v.rowid=f.rowid WHERE v.name=?1 AND v.value IS NOT NULL AND v.index_data IS NOT NULL AND f.key=v.key AND f.field0=json_extract(v.index_data,'$.fields[0]') AND f.field1=json_extract(v.index_data,'$.fields[1]') AND f.tags=json_extract(v.index_data,'$.tags') AND f.ordinal=json_extract(v.index_data,'$.order')"), [name], |row| row.get(0)).map_err(storage_error)?;
    if expected != actual || matched != expected {
        return Err(storage_error("plugin search index is inconsistent"));
    }
    Ok(true)
}

fn valid_tags(tags: &[String]) -> bool {
    tags.len() <= storage::MAX_TAGS
        && tags.iter().all(|tag| {
            !tag.is_empty() && tag.len() <= storage::MAX_TAG_BYTES && !tag.contains('\0')
        })
}

pub(in crate::store) fn validate_index(index: &Index) -> Result<(), Fault> {
    if !valid_tags(&index.tags)
        || index.fields.iter().any(|field| field.contains('\0'))
        || index.source.as_ref().is_some_and(|source| !source.valid())
        || serde_json::to_vec(index).map_or(true, |bytes| bytes.len() > storage::MAX_INDEX_BYTES)
    {
        return Err(sailry_protocol::Fault::invalid(
            "invalid plugin search index",
        ));
    }
    Ok(())
}

fn validate_query(query: &Search) -> Result<(), Fault> {
    if query.terms.len() > storage::MAX_TERMS
        || query
            .query
            .as_ref()
            .is_some_and(|text| text.len() > storage::MAX_QUERY_BYTES || text.contains('\0'))
        || query
            .terms
            .iter()
            .any(|term| term.is_empty() || term.contains('\0'))
        || query.terms.iter().map(String::len).sum::<usize>() > storage::MAX_QUERY_BYTES
        || !valid_tags(&query.all)
        || !valid_tags(&query.any)
        || query.weights.contains(&0)
        || !(1..=storage::MAX_PAGE_LIMIT).contains(&query.limit)
        || usize::from(query.offset) > storage::MAX_NAMESPACE_KEYS
    {
        return Err(sailry_protocol::Fault::invalid(
            "invalid plugin search query",
        ));
    }
    Ok(())
}

/// Called inside the existing admission transaction after the KV row is replaced.
pub(super) fn replace(
    db: &Connection,
    name: &str,
    key: &str,
    index: Option<&Index>,
) -> Result<(), Fault> {
    let table = table(name);
    if !exists(db, &table)? {
        if index.is_none() {
            return Ok(());
        }
        db.execute_batch(&definition(name)).map_err(storage_error)?;
    }
    let rowid: i64 = db
        .query_row(
            "SELECT rowid FROM plugin_values WHERE name=?1 AND key=?2",
            params![name, key],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    db.execute(&format!("DELETE FROM {table} WHERE rowid=?1"), [rowid])
        .map_err(storage_error)?;
    if let Some(index) = index {
        let tags = serde_json::to_string(&index.tags).map_err(storage_error)?;
        db.execute(&format!("INSERT INTO {table}(rowid,field0,field1,cjk0,cjk1,key,tags,ordinal) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)"), params![rowid,index.fields[0],index.fields[1],crate::knowledge::text::index(&index.fields[0]),crate::knowledge::text::index(&index.fields[1]),key,tags,index.order]).map_err(storage_error)?;
    }
    Ok(())
}

pub(super) fn read(db: &Connection, name: &str, query: &Search) -> Result<SearchPage, Fault> {
    validate_query(query)?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(storage_error)?
        .as_millis()
        .try_into()
        .map_err(storage_error)?;
    let mut page = SearchPage {
        entries: Vec::new(),
        next: None,
        now_ms,
        mode: storage::Mode::Keyword,
        indexing: false,
    };
    if !check(db, name)? {
        return Ok(page);
    }
    let table = table(name);
    let sql = statement(&table, query, "v.key,v.revision,v.value");
    let mut statement = db.prepare(&sql).map_err(storage_error)?;
    let parameters = parameters(name, query, i64::from(query.limit) + 1, query.offset)?;
    let mut rows = statement
        .query(rusqlite::params_from_iter(parameters.iter()))
        .map_err(storage_error)?;
    while let Some(row) = rows.next().map_err(storage_error)? {
        if page.entries.len() == usize::from(query.limit) {
            page.next = Some(query.offset + page.entries.len() as u16);
            break;
        }
        let revision: i64 = row.get(1).map_err(storage_error)?;
        let body: Vec<u8> = row.get(2).map_err(storage_error)?;
        let entry = Entry {
            key: row.get(0).map_err(storage_error)?,
            revision: u64::try_from(revision)
                .ok()
                .filter(|revision| *revision > 0)
                .ok_or_else(|| storage_error("invalid plugin value revision"))?,
            value: serde_json::from_slice(&body).map_err(storage_error)?,
            present: true,
        };
        page.entries.push(entry);
        // Include the output envelope, exact SDK revision strings and a possible next offset.
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
    Ok(page)
}

/// Ranked keys keep semantic fusion bounded without loading every JSON value.
pub(in crate::store) fn ranked(
    db: &Connection,
    name: &str,
    query: &Search,
) -> Result<Vec<(String, u64)>, Fault> {
    validate_query(query)?;
    if !check(db, name)? {
        return Ok(Vec::new());
    }
    let sql = statement(&table(name), query, "v.key,v.revision");
    let mut statement = db.prepare(&sql).map_err(storage_error)?;
    let parameters = parameters(name, query, storage::MAX_NAMESPACE_KEYS as i64, 0)?;
    let rows = statement
        .query_map(rusqlite::params_from_iter(parameters.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(storage_error)?;
    rows.map(|row| {
        let (key, revision) = row.map_err(storage_error)?;
        let revision = u64::try_from(revision)
            .ok()
            .filter(|revision| *revision > 0)
            .ok_or_else(|| storage_error("invalid plugin value revision"))?;
        Ok((key, revision))
    })
    .collect()
}

fn statement(table: &str, query: &Search, columns: &str) -> String {
    let (matching, ordering) = if query.terms.is_empty() {
        (String::new(), String::new())
    } else {
        (
            format!("AND {table} MATCH ?2"),
            format!("bm25({table},?5,?6,?5,?6),"),
        )
    };
    format!(
        "SELECT {columns} FROM {table} f JOIN plugin_values v ON v.rowid=f.rowid WHERE v.name=?1 {matching} AND NOT EXISTS(SELECT 1 FROM json_each(?3) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(f.tags) tag WHERE tag.value=wanted.value)) AND (json_array_length(?4)=0 OR EXISTS(SELECT 1 FROM json_each(?4) wanted JOIN json_each(f.tags) tag ON tag.value=wanted.value)) ORDER BY {ordering} f.ordinal DESC,v.key ASC LIMIT ?7 OFFSET ?8"
    )
}

fn parameters(
    name: &str,
    query: &Search,
    limit: i64,
    offset: u16,
) -> Result<[rusqlite::types::Value; 8], Fault> {
    // Quote every term; callers cannot inject MATCH operators or column selectors.
    let terms = query
        .terms
        .iter()
        .map(|term| crate::knowledge::text::phrase(term))
        .collect::<Vec<_>>()
        .join(" OR ");
    Ok([
        name.to_owned().into(),
        terms.into(),
        serde_json::to_string(&query.all)
            .map_err(storage_error)?
            .into(),
        serde_json::to_string(&query.any)
            .map_err(storage_error)?
            .into(),
        i64::from(query.weights[0]).into(),
        i64::from(query.weights[1]).into(),
        limit.into(),
        i64::from(offset).into(),
    ])
}
