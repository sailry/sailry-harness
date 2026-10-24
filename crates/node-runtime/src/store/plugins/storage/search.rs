//! Native FTS over each captured package's private, rebuildable corpus.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};
use storage::{Index, Search, SearchPage};
use turso::{Connection, params};
mod indexes;
pub(in crate::store) use indexes::{apply as rebuild, validate};
#[cfg(test)]
mod tests;

fn table(name: &str) -> String {
    format!("plugin_search_{}", blake3::hash(name.as_bytes()).to_hex())
}

fn definition(name: &str) -> [String; 2] {
    let table = table(name);
    [
        format!(
            "CREATE TABLE {table}(id INTEGER PRIMARY KEY,key TEXT NOT NULL UNIQUE,field0 TEXT NOT NULL,field1 TEXT NOT NULL,cjk0 TEXT NOT NULL,cjk1 TEXT NOT NULL,tags TEXT NOT NULL,ordinal INTEGER NOT NULL)"
        ),
        format!(
            "CREATE INDEX {table}_fts ON {table} USING fts(field0,field1,cjk0,cjk1) WITH(tokenizer='whitespace')"
        ),
    ]
}

/// Current declarations include previously created, now empty corpora.
/// The schema owner reports a missing nullable column before applying its NULL default.
pub(in crate::store) async fn definitions(
    db: &Connection,
    has_index_data: bool,
) -> Result<Vec<String>, Fault> {
    let mut definitions = Vec::new();
    for (name, indexed) in catalog(db, has_index_data).await? {
        if indexed || exists(db, &table(&name)).await? {
            definitions.extend(definition(&name));
        }
    }
    Ok(definitions)
}

async fn catalog(db: &Connection, has_index_data: bool) -> Result<Vec<(String, bool)>, Fault> {
    let sql = if has_index_data {
        "SELECT name,max(index_data IS NOT NULL) FROM plugin_values GROUP BY name ORDER BY name"
    } else {
        "SELECT name,0 FROM plugin_values GROUP BY name ORDER BY name"
    };
    let mut rows = db.query(sql, ()).await.map_err(storage_error)?;
    let mut names = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        names.push((
            row.get(0).map_err(storage_error)?,
            row.get(1).map_err(storage_error)?,
        ));
    }
    Ok(names)
}

async fn exists(db: &Connection, table: &str) -> Result<bool, Fault> {
    db.prepare("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)")
        .await
        .map_err(storage_error)?
        .query_row([table])
        .await
        .map_err(storage_error)?
        .get(0)
        .map_err(storage_error)
}

/// Only first-corpus creation uses Turso's required exclusive schema transaction.
pub(in crate::store) async fn creates(db: &Connection, request: &Request) -> Result<bool, Fault> {
    let index = match &request.command {
        Command::WriteIndexedPluginValue { .. } => true,
        Command::PluginTransaction { operations } => operations.iter().any(|operation| {
            matches!(
                operation,
                sailry_protocol::plugin::transaction::Operation::Index { .. }
            )
        }),
        _ => false,
    };
    if !index {
        return Ok(false);
    }
    let Some(context) = &request.plugin else {
        return Ok(false);
    };
    Ok(!exists(db, &table(&context.package.name)).await?)
}

/// Queries check readiness; startup validates the complete derived corpus.
pub(super) async fn check(db: &Connection, name: &str) -> Result<bool, Fault> {
    let table = table(name);
    if !exists(db, &table).await? {
        let indexed: bool = db
            .prepare("SELECT EXISTS(SELECT 1 FROM plugin_values WHERE name=?1 AND index_data IS NOT NULL)")
            .await
            .map_err(storage_error)?
            .query_row([name])
            .await
            .map_err(storage_error)?
            .get(0)
            .map_err(storage_error)?;
        return if indexed {
            Err(storage_error("plugin search index is missing"))
        } else {
            Ok(false)
        };
    }
    let indexed: bool = db.prepare("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='index' AND name=?1 AND tbl_name=?2)").await.map_err(storage_error)?
        .query_row(params![format!("{table}_fts"), table.as_str()]).await.map_err(storage_error)?
        .get(0).map_err(storage_error)?;
    if !indexed {
        return Err(storage_error("plugin search index is missing"));
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
        return Err(Fault::invalid("invalid plugin search index"));
    }
    Ok(())
}

fn validate_query(query: &Search) -> Result<(), Fault> {
    if query.query.as_ref().is_some_and(|text| text.contains('\0'))
        || query
            .terms
            .iter()
            .any(|term| term.is_empty() || term.contains('\0'))
        || query
            .all
            .iter()
            .any(|tag| tag.is_empty() || tag.contains('\0'))
        || query
            .any
            .iter()
            .any(|tag| tag.is_empty() || tag.contains('\0'))
        || query.weights.contains(&0)
        || query.limit == 0
    {
        return Err(Fault::invalid("invalid plugin search query"));
    }
    Ok(())
}

/// The native index tracks this derived row in the same KV admission transaction.
pub(super) async fn replace(
    db: &Connection,
    name: &str,
    key: &str,
    index: Option<&Index>,
) -> Result<(), Fault> {
    let table = table(name);
    if !exists(db, &table).await? {
        if index.is_none() {
            return Ok(());
        }
        for ddl in definition(name) {
            db.execute_batch(ddl).await.map_err(storage_error)?;
        }
    }
    if let Some(index) = index {
        let tags = serde_json::to_string(&index.tags).map_err(storage_error)?;
        db.execute(&format!("INSERT INTO {table}(key,field0,field1,cjk0,cjk1,tags,ordinal) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(key) DO UPDATE SET field0=excluded.field0,field1=excluded.field1,cjk0=excluded.cjk0,cjk1=excluded.cjk1,tags=excluded.tags,ordinal=excluded.ordinal"),
            params![key, crate::knowledge::text::field(&index.fields[0]), crate::knowledge::text::field(&index.fields[1]), crate::knowledge::text::index(&index.fields[0]), crate::knowledge::text::index(&index.fields[1]), tags, index.order]).await.map_err(storage_error)?;
    } else {
        db.execute(&format!("DELETE FROM {table} WHERE key=?1"), [key])
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) async fn read(db: &Connection, name: &str, query: &Search) -> Result<SearchPage, Fault> {
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
    let expression = expression(query);
    if (!query.terms.is_empty() && expression.is_empty()) || !check(db, name).await? {
        return Ok(page);
    }
    let sql = statement(
        &table(name),
        query,
        "v.key,v.revision,v.value,v.index_data,f.field0,f.field1,f.cjk0,f.cjk1,f.tags,f.ordinal",
    );
    let parameters = parameters(
        name,
        query,
        expression,
        i64::from(query.limit.min(storage::MAX_PAGE_LIMIT)) + 1,
        query.offset,
    )?;
    let mut rows = db.query(&sql, parameters).await.map_err(storage_error)?;
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        if page.entries.len() == usize::from(query.limit.min(storage::MAX_PAGE_LIMIT)) {
            page.next = Some(query.offset.saturating_add(page.entries.len() as u16));
            break;
        }
        let revision: i64 = row.get(1).map_err(storage_error)?;
        indexes::hit(&row, 3)?;
        let body: Vec<u8> = row.get(2).map_err(storage_error)?;
        page.entries.push(Entry {
            key: row.get(0).map_err(storage_error)?,
            revision: revision
                .try_into()
                .ok()
                .filter(|revision| *revision > 0)
                .ok_or_else(|| storage_error("invalid plugin value revision"))?,
            value: serde_json::from_slice(&body).map_err(storage_error)?,
            present: true,
        });
        if serde_json::to_vec(&Output::PluginSearch(page.clone()))
            .map_err(storage_error)?
            .len()
            + page.entries.len() * 2
            + 8
            > storage::MAX_SEARCH_BYTES
        {
            page.entries.pop();
            page.next = Some(query.offset.saturating_add(page.entries.len() as u16));
            break;
        }
    }
    drop(rows);
    Ok(page)
}

/// Semantic fusion receives only ranked keys and revisions, not all JSON values.
pub(in crate::store) async fn ranked(
    db: &Connection,
    name: &str,
    query: &Search,
) -> Result<Vec<(String, u64)>, Fault> {
    validate_query(query)?;
    let expression = expression(query);
    if (!query.terms.is_empty() && expression.is_empty()) || !check(db, name).await? {
        return Ok(Vec::new());
    }
    let sql = statement(
        &table(name),
        query,
        "v.key,v.revision,v.index_data,f.field0,f.field1,f.cjk0,f.cjk1,f.tags,f.ordinal",
    );
    let mut rows = db
        .query(
            &sql,
            parameters(
                name,
                query,
                expression,
                storage::MAX_NAMESPACE_KEYS as i64,
                0,
            )?,
        )
        .await
        .map_err(storage_error)?;
    let mut entries = Vec::new();
    while let Some(row) = rows.next().await.map_err(storage_error)? {
        indexes::hit(&row, 2)?;
        let revision: i64 = row.get(1).map_err(storage_error)?;
        let revision = revision
            .try_into()
            .ok()
            .filter(|revision| *revision > 0)
            .ok_or_else(|| storage_error("invalid plugin value revision"))?;
        entries.push((row.get(0).map_err(storage_error)?, revision));
    }
    Ok(entries)
}

fn expression(query: &Search) -> String {
    let terms = query
        .terms
        .iter()
        .map(|term| crate::knowledge::text::phrase(term))
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>()
        .join(" OR ");
    if terms.is_empty() {
        return String::new();
    }
    ["field0", "field1", "cjk0", "cjk1"]
        .into_iter()
        .enumerate()
        .map(|(index, field)| format!("{field}:({terms})^{}", query.weights[index % 2]))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn statement(table: &str, query: &Search, columns: &str) -> String {
    // In Turso 0.8.3, adding a secondary sort to its direct FTS scan can
    // discard score ordering. Materialize native scores before ACL/tag joins
    // and pagination; no top-N boundary precedes those filters.
    let (candidates, source, ordering) = if query.terms.is_empty() {
        (String::new(), format!("{table} f"), String::new())
    } else {
        (
            format!(
                "WITH candidates AS MATERIALIZED (SELECT id,fts_score(field0,field1,cjk0,cjk1,?2) AS score FROM {table} WHERE fts_match(field0,field1,cjk0,cjk1,?2)) "
            ),
            format!("candidates c JOIN {table} f ON f.id=c.id"),
            "c.score DESC,".into(),
        )
    };
    format!(
        "{candidates}SELECT {columns} FROM {source} JOIN plugin_values v ON v.name=?1 AND v.key=f.key WHERE v.value IS NOT NULL AND v.index_data IS NOT NULL AND NOT EXISTS(SELECT 1 FROM json_each(?3) wanted WHERE NOT EXISTS(SELECT 1 FROM json_each(f.tags) tag WHERE tag.value=wanted.value)) AND (json_array_length(?4)=0 OR EXISTS(SELECT 1 FROM json_each(?4) wanted JOIN json_each(f.tags) tag ON tag.value=wanted.value)) ORDER BY {ordering}f.ordinal DESC,v.key ASC LIMIT ?5 OFFSET ?6"
    )
}

fn parameters(
    name: &str,
    query: &Search,
    expression: String,
    limit: i64,
    offset: u16,
) -> Result<[turso::Value; 6], Fault> {
    Ok([
        name.to_owned().into(),
        expression.into(),
        serde_json::to_string(&query.all)
            .map_err(storage_error)?
            .into(),
        serde_json::to_string(&query.any)
            .map_err(storage_error)?
            .into(),
        limit.into(),
        i64::from(offset).into(),
    ])
}
