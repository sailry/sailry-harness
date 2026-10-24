//! Configuration changes validate public choices against their owning declaration.
use super::*;
use sailry_protocol::SessionConfig;

pub(in crate::store) async fn validate(
    db: &Connection,
    previous: Option<&SessionConfig>,
    config: &SessionConfig,
    caller: Option<&plugin::Reference>,
) -> Result<(), Fault> {
    let names: std::collections::BTreeSet<_> = config
        .options
        .keys()
        .chain(
            previous
                .into_iter()
                .flat_map(|config| config.options.keys()),
        )
        .collect();
    for name in names {
        let before = previous.and_then(|config| config.options.get(name));
        let after = config.options.get(name);
        if before == after {
            continue;
        }
        if caller.is_some_and(|caller| caller.name != *name) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "conversation options belong to another package",
            ));
        }
        let Some(info) = get(db, name).await? else {
            if caller.is_none() {
                continue;
            }
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "conversation option package is unavailable",
            ));
        };
        if caller.is_some_and(|caller| *caller != info.summary.reference()) {
            return Err(Fault::new(
                ErrorCode::RevisionConflict,
                "conversation option package changed",
            ));
        }
        let schema = info
            .extension
            .as_ref()
            .map(crate::plugins::options::schema)
            .transpose()?
            .flatten();
        let Some(schema) = schema else {
            if caller.is_none() {
                continue;
            }
            return Err(Fault::invalid("package has no conversation options"));
        };
        let extension = info
            .extension
            .as_ref()
            .expect("option schema has an extension");
        for key in before
            .into_iter()
            .chain(after)
            .flat_map(|values| values.keys())
        {
            if !schema.properties.contains_key(key)
                && before.and_then(|values| values.get(key))
                    != after.and_then(|values| values.get(key))
            {
                return Err(Fault::invalid("conversation option was not declared"));
            }
        }
        let declared = after.map(|values| {
            values
                .iter()
                .filter(|(key, _)| schema.properties.contains_key(*key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        });
        crate::plugins::options::project(extension, declared.as_ref())?;
    }
    Ok(())
}

pub(super) async fn check(db: &Connection, info: &Info) -> Result<(), Fault> {
    let Some(extension) = &info.extension else {
        return Ok(());
    };
    let Some(schema) = crate::plugins::options::schema(extension)? else {
        return Ok(());
    };
    let imports = schema
        .properties
        .values()
        .filter_map(|field| field.import.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    if imports.is_empty() {
        return Ok(());
    }
    for summary in list(db)
        .await?
        .into_iter()
        .filter(|summary| summary.name != info.summary.name)
    {
        let info = required(db, &summary.name).await?;
        if let Some(extension) = &info.extension
            && let Some(schema) = crate::plugins::options::schema(extension)?
            && schema
                .properties
                .values()
                .filter_map(|field| field.import.as_ref())
                .any(|source| imports.contains(source))
        {
            return Err(Fault::invalid(
                "conversation option import already has an installed owner",
            ));
        }
    }
    Ok(())
}

/// Unknown retired fields remain private source data until their package declares an import.
pub(in crate::store) fn preserve(
    previous: Option<&[u8]>,
    config: &SessionConfig,
) -> Result<Vec<u8>, Fault> {
    let mut next = serde_json::to_value(config).map_err(storage_error)?;
    if let Some(previous) = previous {
        let previous: serde_json::Value =
            serde_json::from_slice(previous).map_err(storage_error)?;
        let known: SessionConfig =
            serde_json::from_value(previous.clone()).map_err(storage_error)?;
        let known = serde_json::to_value(known).map_err(storage_error)?;
        if let (Some(previous), Some(known), Some(next)) = (
            previous.as_object(),
            known.as_object(),
            next.as_object_mut(),
        ) {
            for (key, value) in previous {
                if !known.contains_key(key) {
                    next.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
    }
    encode(&next)
}

pub(in crate::store) async fn migrate(db: &Connection, info: &Info) -> Result<Vec<Event>, Fault> {
    let Some(extension) = &info.extension else {
        return Ok(vec![]);
    };
    let Some(schema) = crate::plugins::options::schema(extension)? else {
        return Ok(vec![]);
    };
    let mut changes = vec![];
    for (key, field) in &schema.properties {
        let Some(source) = &field.import else {
            continue;
        };
        let source_path = format!("$.\"{source}\"");
        let target_path = format!("$.options.\"{}\".\"{key}\"", info.summary.name);
        let mut after = i64::MIN;
        loop {
            let row = {
                let mut statement = db.prepare("SELECT s.rowid,s.id,r.config FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision WHERE s.rowid>=?1 AND json_type(r.config,?2) IS NOT NULL AND json_type(r.config,?3) IS NULL AND NOT EXISTS(SELECT 1 FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=s.id) ORDER BY s.rowid LIMIT 1").await.map_err(storage_error)?;
                match statement
                    .query_row(params![after, source_path.as_str(), target_path.as_str()])
                    .await
                {
                    Ok(row) => {
                        let mapped: turso::Result<_> = (|row: &turso::Row| {
                            Ok((
                                row.get::<i64>(0)?,
                                row.get::<String>(1)?,
                                row.get::<Vec<u8>>(2)?,
                            ))
                        })(&row);
                        Some(mapped.map_err(storage_error)?)
                    }
                    Err(turso::Error::QueryReturnedNoRows) => None,
                    Err(error) => return Err(storage_error(error)),
                }
            };
            let Some((row, id, bytes)) = row else { break };
            let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(storage_error)?;
            let mut session =
                super::super::commands::session(db, id.parse().map_err(storage_error)?).await?;
            session
                .config
                .options
                .entry(info.summary.name.clone())
                .or_default()
                .insert(key.clone(), raw[source].clone());
            crate::plugins::options::captured(info, &session.config)?;
            let (_, event) = super::super::commands::revise_session(db, session).await?;
            changes.extend(event);
            let Some(next) = row.checked_add(1) else {
                break;
            };
            after = next;
        }
        let previous: Option<Vec<u8>> = {
            let mut statement = db.prepare("SELECT config FROM defaults WHERE singleton=1 AND json_type(config,?1) IS NOT NULL AND json_type(config,?2) IS NULL").await.map_err(storage_error)?;
            match statement.query_row(params![source_path, target_path]).await {
                Ok(row) => {
                    let mapped: turso::Result<_> = row.get(0);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        if let Some(bytes) = previous {
            let raw: serde_json::Value = serde_json::from_slice(&bytes).map_err(storage_error)?;
            let mut defaults = super::super::commands::defaults(db).await?;
            let config = defaults
                .config
                .as_mut()
                .ok_or_else(|| Fault::invalid("default conversation configuration is missing"))?;
            config
                .options
                .entry(info.summary.name.clone())
                .or_default()
                .insert(key.clone(), raw[source].clone());
            crate::plugins::options::captured(info, config)?;
            defaults.revision = defaults
                .revision
                .checked_add(1)
                .filter(|value| *value <= i64::MAX as u64)
                .ok_or_else(|| Fault::invalid("configuration revision exhausted"))?;
            db.execute(
                "UPDATE defaults SET revision=?1,config=?2 WHERE singleton=1",
                params![defaults.revision as i64, preserve(Some(&bytes), config)?],
            )
            .await
            .map_err(storage_error)?;
            changes.push(Event::DefaultsChanged(defaults));
        }
    }
    Ok(changes)
}

#[cfg(test)]
mod tests;
