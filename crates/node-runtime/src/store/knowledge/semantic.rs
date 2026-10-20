//! Manifest opt-in registers derived recall within one captured private namespace.
use super::*;
use crate::store::plugins::storage::{self as values, search};
use sailry_link::CancellationToken;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct Plan {
    pub(crate) package: plugin::Reference,
    pub(crate) collections: Vec<Stored>,
    pub(crate) keyword: Vec<(String, u64)>,
    pub(crate) eligible: BTreeSet<String>,
    pub(crate) settings_revision: u64,
}

pub(crate) struct Stamp {
    pub(crate) collection: KnowledgeId,
    pub(crate) key: String,
    pub(crate) revision: String,
}

pub(crate) struct Selection {
    pub(crate) keys: Vec<String>,
    pub(crate) stamps: Vec<Stamp>,
    pub(crate) mode: knowledge::Mode,
    pub(crate) indexing: bool,
}

pub(crate) struct Materialized {
    pub(crate) page: plugin::storage::SearchPage,
    pub(crate) stale: bool,
}

pub(in crate::store) fn prepare(
    db: &Connection,
    request: &Request,
    stop: &CancellationToken,
) -> Result<Plan, Fault> {
    check(stop)?;
    let context = request
        .plugin
        .as_ref()
        .ok_or_else(|| denied("plugin provenance is required"))?;
    sources::package(db, context)?;
    let Command::SearchPluginValues(query) = &request.command else {
        return Err(Fault::invalid("plugin storage search expected"));
    };
    let collections = if query.mode != knowledge::Mode::Keyword
        && query
            .query
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
    {
        selected(db, &context.package, stop)?
    } else {
        vec![]
    };
    let lexical = lexical(request);
    let Command::SearchPluginValues(keyword_query) = &lexical.command else {
        unreachable!()
    };
    let keyword = search::ranked(db, &context.package.name, keyword_query)?;
    let eligible = eligible(db, &context.package, &collections, stop)?;
    Ok(Plan {
        package: context.package.clone(),
        collections,
        keyword,
        eligible,
        settings_revision: super::super::local_models::settings(db)?.revision,
    })
}

fn selected(
    db: &Connection,
    reference: &plugin::Reference,
    stop: &CancellationToken,
) -> Result<Vec<Stored>, Fault> {
    let declarations = declarations(db, reference)?;
    let stored = collections::load(db)?;
    let mut selected = Vec::new();
    for declaration in declarations {
        check(stop)?;
        if let Some(entry) = stored
            .iter()
            .find(|stored| stored.collection.id == identifier(&reference.name, &declaration))
        {
            if !automatic(entry) {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "semantic registration identifier is already owned",
                ));
            }
            selected.push(entry.clone());
        }
    }
    Ok(selected)
}

pub(in crate::store) fn startup(db: &Connection, stop: &CancellationToken) -> Result<bool, Fault> {
    let mut changed = false;
    for summary in super::super::plugins::list(db)? {
        check(stop)?;
        if summary.enabled {
            changed |= ensure(db, &summary.reference(), stop)?.1;
        }
    }
    Ok(changed)
}

/// Completion observation runs this before a successful storage receipt is returned.
pub(in crate::store) fn changed(
    db: &Connection,
    request: &Request,
    stop: &CancellationToken,
) -> Result<bool, Fault> {
    let Some(context) = &request.plugin else {
        return Ok(false);
    };
    let (_, mut changed) = ensure(db, &context.package, stop)?;
    for mut stored in collections::load(db)? {
        check(stop)?;
        if automatic(&stored)
            && stored
                .capture
                .package
                .as_ref()
                .is_some_and(|package| package.name == context.package.name)
            && stored.collection.progress.status != knowledge::Status::Pending
        {
            collections::advance(&mut stored)?;
            collections::persist(db, &stored)?;
            changed = true;
        }
    }
    Ok(changed)
}

fn ensure(
    db: &Connection,
    reference: &plugin::Reference,
    stop: &CancellationToken,
) -> Result<(Vec<Stored>, bool), Fault> {
    let declarations = declarations(db, reference)?;
    let mut stored = collections::load(db)?;
    let mut selected = Vec::new();
    let mut changed = false;
    for declaration in declarations {
        check(stop)?;
        let prefix = selector(&declaration);
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM plugin_values WHERE name=?1 AND value IS NOT NULL AND index_data IS NOT NULL AND (?2=1 AND key=?3 OR ?2=0 AND substr(key,1,length(?3))=?3) AND json_type(index_data,'$.source')='object')",
            params![reference.name, declaration.key.is_some(), prefix], |row| row.get(0),
        ).map_err(storage_error)?;
        if !exists {
            continue;
        }
        let id = identifier(&reference.name, &declaration);
        if let Some(previous) = stored.iter_mut().find(|stored| stored.collection.id == id) {
            if !automatic(previous) {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "semantic registration identifier is already owned",
                ));
            }
            let latest = super::super::plugins::get(db, &reference.name)?;
            // An old admitted turn retains access without replacing a newer package capture.
            if previous.capture.package.as_ref() != Some(reference)
                && latest
                    .as_ref()
                    .is_some_and(|info| info.summary.reference() == *reference)
            {
                previous.capture.package = Some(reference.clone());
                collections::advance(previous)?;
                collections::persist(db, previous)?;
                changed = true;
            }
            selected.push(previous.clone());
        } else {
            let entry = Stored {
                collection: knowledge::Collection {
                    id,
                    revision: 1,
                    paused: false,
                    rebuild: 0,
                    owner: Owner::Plugin {
                        name: reference.name.clone(),
                    },
                    definition: Definition {
                        name: format!("{} storage", reference.name),
                        source: Source::Storage {
                            scope: Scope::Node,
                            prefix: prefix.into(),
                        },
                        model: None,
                        globs: vec![],
                        tags: vec![],
                        shared_with: vec![],
                    },
                    progress: knowledge::Progress {
                        generation: 1,
                        ..Default::default()
                    },
                },
                capture: Capture {
                    package: Some(reference.clone()),
                    worktree: None,
                    session: None,
                },
                automatic: true,
            };
            collections::persist(db, &entry)?;
            selected.push(entry.clone());
            stored.push(entry);
            changed = true;
        }
    }
    Ok((selected, changed))
}

fn declarations(
    db: &Connection,
    reference: &plugin::Reference,
) -> Result<Vec<plugin::storage::Collection>, Fault> {
    let package = super::super::agent::plugins::packages(db, std::slice::from_ref(reference))?
        .pop()
        .ok_or_else(|| Fault::new(ErrorCode::NotConfigured, "captured plugin is unavailable"))?;
    Ok(package
        .extension
        .filter(|extension| extension.actions.contains(&plugin::Action::ReadStorage))
        .and_then(|extension| extension.storage)
        .map(|storage| storage.collections)
        .unwrap_or_default()
        .into_iter()
        .filter(|collection| {
            collection.scope == Scope::Node
                && collection
                    .index
                    .as_ref()
                    .is_some_and(|index| index.semantic)
        })
        .collect())
}

fn selector(collection: &plugin::storage::Collection) -> &str {
    collection
        .key
        .as_deref()
        .or(collection.prefix.as_deref())
        .expect("validated storage selector")
}

fn identifier(name: &str, collection: &plugin::storage::Collection) -> KnowledgeId {
    let mut hash = blake3::Hasher::new_derive_key("Sailry semantic storage collection v1");
    hash.update(&(name.len() as u64).to_le_bytes());
    hash.update(name.as_bytes());
    hash.update(b"node");
    hash.update(&[u8::from(collection.key.is_some())]);
    hash.update(selector(collection).as_bytes());
    let mut bytes: [u8; 16] = hash.finalize().as_bytes()[..16].try_into().unwrap();
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    KnowledgeId::try_from(uuid::Uuid::from_bytes(bytes)).expect("derived identifier is not nil")
}

pub(in crate::store) fn automatic(stored: &Stored) -> bool {
    if !stored.automatic {
        return false;
    }
    let (
        Owner::Plugin { name },
        Source::Storage {
            scope: Scope::Node,
            prefix,
        },
    ) = (
        &stored.collection.owner,
        &stored.collection.definition.source,
    )
    else {
        return false;
    };
    [false, true].into_iter().any(|key| {
        let declaration = plugin::storage::Collection {
            scope: Scope::Node,
            key: key.then(|| prefix.clone()),
            prefix: (!key).then(|| prefix.clone()),
            schema: serde_json::Value::Bool(true),
            index: None,
        };
        identifier(name, &declaration) == stored.collection.id
    })
}

pub(in crate::store) fn sources(
    db: &Connection,
    stored: &Stored,
    after: Option<&str>,
    limit: u16,
    stop: &CancellationToken,
) -> Result<super::storage::PageRaw, Fault> {
    check(stop)?;
    if !(1..=16).contains(&limit)
        || after.is_some_and(|key| {
            key.is_empty() || key.len() > plugin::storage::MAX_KEY_BYTES || key.contains('\0')
        })
    {
        return Err(Fault::invalid("invalid automatic storage source page"));
    }
    let declaration = captured(db, stored)?;
    let reference = stored
        .capture
        .package
        .as_ref()
        .expect("validated automatic capture");
    let namespace = values::semantic::Namespace::from_reference(db, reference)?;
    let mut statement = db.prepare(
        "SELECT key FROM plugin_values WHERE name=?1 AND value IS NOT NULL AND index_data IS NOT NULL AND (?2=1 AND key=?3 OR ?2=0 AND substr(key,1,length(?3))=?3) AND (?4 IS NULL OR key>?4) ORDER BY key LIMIT ?5"
    ).map_err(storage_error)?;
    let mut rows = statement
        .query(params![
            reference.name,
            declaration.key.is_some(),
            selector(&declaration),
            after,
            i64::from(limit) + 1
        ])
        .map_err(storage_error)?;
    let mut page = super::storage::PageRaw {
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
        if let Some(record) = values::semantic::record(db, &namespace, &key)? {
            page.records.push(convert(record)?);
        }
        last = Some(key);
        scanned += 1;
    }
    Ok(page)
}

pub(in crate::store) fn source(
    db: &Connection,
    stored: &Stored,
    key: &str,
    stop: &CancellationToken,
) -> Result<Option<super::storage::Record>, Fault> {
    check(stop)?;
    let declaration = match captured(db, stored) {
        Ok(declaration) => declaration,
        Err(error) if error.code == ErrorCode::RevisionConflict => return Ok(None),
        Err(error) => return Err(error),
    };
    if !declaration.matches(Scope::Node, key) {
        return Ok(None);
    }
    let reference = stored
        .capture
        .package
        .as_ref()
        .expect("validated automatic capture");
    let namespace = values::semantic::Namespace::from_reference(db, reference)?;
    values::semantic::record(db, &namespace, key)?
        .map(convert)
        .transpose()
}

pub(in crate::store) fn unchanged(
    db: &Connection,
    stored: &Stored,
    record: &super::storage::Record,
    stop: &CancellationToken,
) -> Result<bool, Fault> {
    Ok(
        source(db, stored, &record.key, stop)?.is_some_and(|current| {
            current.revision == record.revision
                && current.hash == record.hash
                && current.source == record.source
                && current.source_revision == record.source_revision
                && current.source_hash == record.source_hash
                && current.title == record.title
                && current.body == record.body
                && current.tags == record.tags
        }),
    )
}

fn captured(db: &Connection, stored: &Stored) -> Result<plugin::storage::Collection, Fault> {
    if !automatic(stored) {
        return Err(Fault::invalid("automatic storage collection expected"));
    }
    let current = collections::load(db)?.into_iter().any(|current| {
        current.automatic == stored.automatic
            && current.collection.id == stored.collection.id
            && current.collection.revision == stored.collection.revision
            && current.collection.rebuild == stored.collection.rebuild
            && current.collection.paused == stored.collection.paused
            && current.collection.progress.generation == stored.collection.progress.generation
            && current.collection.definition == stored.collection.definition
            && current.capture.package == stored.capture.package
    });
    if !current {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "automatic storage collection changed",
        ));
    }
    let reference = stored
        .capture
        .package
        .as_ref()
        .ok_or_else(|| Fault::invalid("automatic storage capture is missing"))?;
    declarations(db, reference)?
        .into_iter()
        .find(|declaration| identifier(&reference.name, declaration) == stored.collection.id)
        .ok_or_else(|| denied("captured package no longer declares this semantic collection"))
}

fn convert(record: values::semantic::Record) -> Result<super::storage::Record, Fault> {
    let body = serde_json::to_vec(&record.entry.value).map_err(storage_error)?;
    let index = serde_json::to_vec(&record.index).map_err(storage_error)?;
    let mut hash = blake3::Hasher::new();
    hash.update(&(body.len() as u64).to_le_bytes());
    hash.update(&body);
    hash.update(&index);
    Ok(super::storage::Record {
        key: record.entry.key,
        revision: record.entry.revision,
        hash: *hash.finalize().as_bytes(),
        title: record.title,
        body: record.body,
        source: record.source,
        source_revision: record.source_revision,
        source_hash: record.source_hash,
        tags: record.index.tags,
    })
}

fn eligible(
    db: &Connection,
    reference: &plugin::Reference,
    collections: &[Stored],
    stop: &CancellationToken,
) -> Result<BTreeSet<String>, Fault> {
    if collections.is_empty() {
        return Ok(BTreeSet::new());
    }
    let declarations = declarations(db, reference)?
        .into_iter()
        .filter(|declaration| {
            collections
                .iter()
                .any(|stored| stored.collection.id == identifier(&reference.name, declaration))
        })
        .collect::<Vec<_>>();
    let mut statement = db.prepare(
        "SELECT v.key FROM plugin_values v WHERE v.name=?1 AND v.value IS NOT NULL AND v.index_data IS NOT NULL AND json_type(v.index_data,'$.source')='object' AND EXISTS(SELECT 1 FROM plugin_values original WHERE original.name=v.name AND original.key=json_extract(v.index_data,'$.source.key') AND original.value IS NOT NULL) ORDER BY v.key LIMIT ?2"
    ).map_err(storage_error)?;
    let rows = statement
        .query_map(
            params![reference.name, plugin::storage::MAX_NAMESPACE_KEYS as i64],
            |row| row.get::<_, String>(0),
        )
        .map_err(storage_error)?;
    let mut keys = BTreeSet::new();
    for row in rows {
        check(stop)?;
        let key = row.map_err(storage_error)?;
        if declarations
            .iter()
            .any(|declaration| declaration.matches(Scope::Node, &key))
        {
            keys.insert(key);
        }
    }
    Ok(keys)
}

pub(in crate::store) fn materialize(
    db: &Connection,
    request: &Request,
    plan: &Plan,
    selection: &Selection,
    stop: &CancellationToken,
) -> Result<Materialized, Fault> {
    check(stop)?;
    let context = request
        .plugin
        .as_ref()
        .ok_or_else(|| denied("plugin provenance is required"))?;
    sources::package(db, context)?;
    if context.package != plan.package {
        return Err(denied("semantic plan belongs to another capture"));
    }
    if selection.mode != knowledge::Mode::Keyword
        && super::super::local_models::settings(db)?.revision != plan.settings_revision
    {
        return fallback(db, request);
    }
    let Command::SearchPluginValues(query) = &request.command else {
        return Err(Fault::invalid("plugin storage search expected"));
    };
    let keyword = plan
        .keyword
        .iter()
        .map(|(key, revision)| (key.as_str(), *revision))
        .collect::<BTreeMap<_, _>>();
    let stamps = selection
        .stamps
        .iter()
        .map(|stamp| (stamp.key.as_str(), stamp))
        .collect::<BTreeMap<_, _>>();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(storage_error)?
        .as_millis()
        .try_into()
        .map_err(storage_error)?;
    let mut page = plugin::storage::SearchPage {
        entries: vec![],
        next: None,
        now_ms,
        mode: selection.mode,
        indexing: selection.indexing,
    };
    for key in selection.keys.iter().skip(usize::from(query.offset)) {
        check(stop)?;
        if page.entries.len() == usize::from(query.limit.min(plugin::storage::MAX_PAGE_LIMIT)) {
            page.next = Some(query.offset.saturating_add(page.entries.len() as u16));
            break;
        }
        if let Some(stamp) = stamps.get(key.as_str()) {
            let Some(stored) = plan
                .collections
                .iter()
                .find(|stored| stored.collection.id == stamp.collection)
            else {
                return Err(denied(
                    "semantic result is outside the captured collections",
                ));
            };
            let record = match super::storage::record(db, stored, key, stop) {
                Ok(record) => record,
                Err(error)
                    if matches!(
                        error.code,
                        ErrorCode::RevisionConflict | ErrorCode::InvalidRequest
                    ) =>
                {
                    return fallback(db, request);
                }
                Err(error) => return Err(error),
            };
            let Some(record) = record else {
                return fallback(db, request);
            };
            if identity(&record)? != stamp.revision {
                return fallback(db, request);
            }
            if !query.all.iter().all(|tag| record.tags.contains(tag))
                || !query.any.is_empty() && !query.any.iter().any(|tag| record.tags.contains(tag))
            {
                return fallback(db, request);
            }
        } else if !keyword.contains_key(key.as_str()) {
            return Err(denied("semantic result has no captured ranking"));
        }
        let mut read = request.clone();
        read.command = Command::ReadPluginValue { key: key.clone() };
        let (Output::PluginValue(entry), _) = super::super::plugins::execute_storage(db, &read)?
        else {
            return Err(Fault::new(ErrorCode::Internal, "plugin entry expected"));
        };
        if !entry.present
            || keyword
                .get(key.as_str())
                .is_some_and(|revision| *revision != entry.revision)
        {
            return fallback(db, request);
        }
        page.entries.push(entry);
        if serde_json::to_vec(&Output::PluginSearch(page.clone()))
            .map_err(storage_error)?
            .len()
            + page.entries.len() * 2
            + 8
            > plugin::storage::MAX_SEARCH_BYTES
        {
            page.entries.pop();
            page.next = Some(query.offset.saturating_add(page.entries.len() as u16));
            break;
        }
    }
    Ok(Materialized { page, stale: false })
}

fn fallback(db: &Connection, request: &Request) -> Result<Materialized, Fault> {
    let (Output::PluginSearch(mut page), _) =
        super::super::plugins::execute_storage(db, &lexical(request))?
    else {
        return Err(Fault::new(ErrorCode::Internal, "plugin search expected"));
    };
    page.mode = knowledge::Mode::Keyword;
    page.indexing = true;
    Ok(Materialized { page, stale: true })
}

fn lexical(request: &Request) -> Request {
    let mut lexical = request.clone();
    if let Command::SearchPluginValues(query) = &mut lexical.command
        && query.terms.is_empty()
        && let Some(original) = query.query.as_ref().filter(|text| !text.trim().is_empty())
    {
        query.terms.push(original.clone());
    }
    lexical
}

fn identity(record: &super::storage::Record) -> Result<String, Fault> {
    Ok(blake3::hash(
        &serde_json::to_vec(&(record.hash, record.source_hash)).map_err(storage_error)?,
    )
    .to_hex()
    .to_string())
}

fn check(stop: &CancellationToken) -> Result<(), Fault> {
    if stop.is_cancelled() {
        Err(Fault::new(
            ErrorCode::Cancelled,
            "semantic storage search was cancelled",
        ))
    } else {
        Ok(())
    }
}
