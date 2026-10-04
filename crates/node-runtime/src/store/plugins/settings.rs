//! Immutable configuration revisions share plugin inventory transactions and receipts.
use super::*;
use crate::plugins::settings::Resolved;
use sailry_protocol::{Secret, plugin::settings::*};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Stored {
    schema: Option<Schema>,
    #[serde(default)]
    values: BTreeMap<String, Value>,
    #[serde(default)]
    pub(super) slots: BTreeMap<String, Option<Secret>>,
    #[serde(default)]
    pub(super) authorizations: BTreeMap<String, super::grants::Grant>,
    #[serde(default)]
    pub(super) mcp: Option<plugin::mcp::Configuration>,
}

pub(super) fn current(db: &Connection, package: &plugin::Reference) -> Result<Info, Fault> {
    let info = required(db, &package.name)?;
    if info.summary.reference() != *package {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin configuration changed",
        ));
    }
    Ok(info)
}

pub(super) fn stored(db: &Connection, info: &Info) -> Result<Stored, Fault> {
    if info.summary.settings_revision == 0 {
        return Ok(Stored {
            schema: info.settings.clone(),
            values: defaults(info.settings.as_ref()),
            slots: BTreeMap::new(),
            authorizations: BTreeMap::new(),
            mcp: None,
        });
    }
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2",
            params![info.summary.name, info.summary.settings_revision as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    serde_json::from_slice(&body.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "plugin configuration revision is unavailable",
        )
    })?)
    .map_err(storage_error)
}

pub(in crate::store) fn validate_revision(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<(), Fault> {
    if package.settings_revision == 0 {
        return Ok(());
    }
    if package.settings_revision > i64::MAX as u64 {
        return Err(invalid("invalid plugin configuration revision"));
    }
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM plugin_settings WHERE plugin=?1 AND revision=?2)",
            params![package.name, package.settings_revision as i64],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin configuration revision is unavailable",
        ))
    }
}

fn defaults(schema: Option<&Schema>) -> BTreeMap<String, Value> {
    schema
        .into_iter()
        .flat_map(|schema| &schema.properties)
        .filter(|(_, field)| field.secret.is_none())
        .filter_map(|(name, field)| field.default.clone().map(|value| (name.clone(), value)))
        .collect()
}

fn same_binding(previous: &Stored, schema: &Schema, name: &str) -> bool {
    let previous = previous
        .schema
        .as_ref()
        .and_then(|schema| schema.properties.get(name))
        .and_then(|field| field.secret.as_ref());
    let current = schema
        .properties
        .get(name)
        .and_then(|field| field.secret.as_ref());
    matches!((previous, current), (Some(previous), Some(current))
        if !previous.unsupported && !current.unsupported && previous == current)
}

fn project(_db: &Connection, info: &Info, stored: &Stored) -> Result<State, Fault> {
    let schema = info.settings.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no valid settings form",
        )
    })?;
    let mut values = defaults(Some(schema));
    // Saved omissions remain unset; only new fields receive draft defaults.
    if let Some(previous) = &stored.schema {
        values.retain(|name, _| !previous.properties.contains_key(name));
    }
    for (name, value) in &stored.values {
        if schema
            .properties
            .get(name)
            .is_some_and(|field| field.secret.is_none())
        {
            values.insert(name.clone(), value.clone());
        }
    }
    let mut configured = Vec::new();
    for (name, value) in &stored.slots {
        if same_binding(stored, schema, name) && value.is_some() {
            configured.push(name.clone());
        }
    }
    let ready = stored.schema.as_ref() == Some(schema)
        && validate_values(schema, &values).is_ok()
        && schema
            .required
            .iter()
            .all(|name| schema.properties[name].secret.is_none() || configured.contains(name));
    Ok(State {
        package: info.summary.reference(),
        values,
        configured,
        keepable: schema
            .properties
            .iter()
            .filter(|(name, field)| field.secret.is_some() && same_binding(stored, schema, name))
            .map(|(name, _)| name.clone())
            .collect(),
        ready,
    })
}

pub(in crate::store) fn read(db: &Connection, package: &plugin::Reference) -> Result<State, Fault> {
    let info = current(db, package)?;
    project(db, &info, &stored(db, &info)?)
}

pub(in crate::store) fn read_secret(
    db: &Connection,
    package: &plugin::Reference,
    field: &str,
) -> Result<Option<sailry_protocol::Secret>, Fault> {
    let info = current(db, package)?;
    let saved = stored(db, &info)?;
    let schema = info
        .settings
        .as_ref()
        .ok_or_else(|| invalid("plugin has no settings form"))?;
    if !schema
        .properties
        .get(field)
        .is_some_and(|field| field.secret.is_some())
    {
        return Err(invalid("setting is not a declared secret slot"));
    }
    if !same_binding(&saved, schema, field) {
        return Err(invalid("setting binding changed"));
    }
    Ok(saved.slots.get(field).cloned().flatten())
}

pub(in crate::store) fn read_admitted(db: &Connection, info: &Info) -> Result<State, Fault> {
    project(db, info, &stored(db, info)?)
}

pub(in crate::store) fn save(
    db: &Connection,
    package: &plugin::Reference,
    values: &BTreeMap<String, Value>,
    updates: &BTreeMap<String, SecretUpdate>,
) -> Result<State, Fault> {
    let mut info = current(db, package)?;
    let schema = info.settings.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no valid settings form",
        )
    })?;
    let previous = stored(db, &info)?;
    validate_values(schema, values)?;
    let models = if schema.properties.values().any(|field| field.model) {
        super::models::catalog(db)?.models
    } else {
        vec![]
    };
    for (name, field) in &schema.properties {
        if field.model
            && let Some(value) = values.get(name).and_then(Value::as_str)
            && !models.iter().any(|model| model.id == value)
        {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "selected plugin model is unavailable",
            ));
        }
        if let Some(model) = &field.effort
            && let Some(value) = values
                .get(name)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        {
            let selected = values
                .get(model)
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("reasoning requires a selected model"))?;
            let model = models
                .iter()
                .find(|model| model.id == selected)
                .ok_or_else(|| invalid("reasoning requires an available model"))?;
            if !model
                .efforts
                .iter()
                .any(|effort| plugin::models::effort_key(*effort) == value)
            {
                return Err(invalid("reasoning choice is not configured for this model"));
            }
        }
    }
    let size = serde_json::to_vec(&(values, updates))
        .map_err(storage_error)?
        .len();
    if size > MAX_BYTES {
        return Err(invalid("plugin settings exceed the size limit"));
    }
    let fields: BTreeMap<_, _> = schema
        .properties
        .iter()
        .filter(|(_, field)| field.secret.is_some())
        .collect();
    if updates.len() != fields.len() || updates.keys().any(|name| !fields.contains_key(name)) {
        return Err(invalid(
            "each secret setting requires an explicit update intent",
        ));
    }
    for (name, update) in updates {
        match update {
            SecretUpdate::Replace(value) => {
                if value.expose().is_empty() || value.expose().contains('\0') {
                    return Err(invalid(
                        "secret replacement must be nonempty and contain no NUL",
                    ));
                }
                validate_value(fields[name], &Value::String(value.expose().into()))?;
                if fields[name]
                    .secret
                    .as_ref()
                    .is_some_and(|binding| binding.header.is_some())
                {
                    reqwest::header::HeaderValue::from_str(value.expose())
                        .map_err(|_| invalid("invalid secret header value"))?;
                }
            }
            SecretUpdate::Keep if !same_binding(&previous, schema, name) => {
                return Err(invalid(
                    "changed secret bindings require replacement or clearing",
                ));
            }
            _ => {}
        }
    }
    let mut slots = BTreeMap::new();
    for (name, update) in updates {
        match update {
            SecretUpdate::Keep => {
                if let Some(value) = previous.slots.get(name) {
                    slots.insert(name.clone(), value.clone());
                }
            }
            SecretUpdate::Replace(value) => {
                slots.insert(name.clone(), Some(value.clone()));
            }
            SecretUpdate::Clear => {
                edit_revisions(db, &package.name, |saved| {
                    if let Some(value) = saved.slots.get_mut(name) {
                        *value = None;
                    }
                })?;
                slots.insert(name.clone(), None);
            }
        }
    }
    let saved = Stored {
        schema: Some(schema.clone()),
        values: values.clone(),
        slots,
        authorizations: previous.authorizations,
        mcp: previous.mcp,
    };
    persist(db, &mut info, &saved)?;
    project(db, &info, &saved)
}

pub(super) fn persist(db: &Connection, info: &mut Info, saved: &Stored) -> Result<(), Fault> {
    let next: i64 = db
        .query_row(
            "SELECT settings_revision FROM plugins WHERE name=?1",
            [&info.summary.name],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    info.summary.settings_revision =
        next.checked_add(1)
            .ok_or_else(|| invalid("plugin configuration revision exhausted"))? as u64;
    db.execute(
        "INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,?2,?3)",
        params![
            info.summary.name,
            info.summary.settings_revision as i64,
            encode(saved)?
        ],
    )
    .map_err(storage_error)?;
    // The inventory tombstone retains this counter when unused revisions are collected.
    db.execute(
        "UPDATE plugins SET settings_revision=?2 WHERE name=?1",
        params![info.summary.name, info.summary.settings_revision as i64],
    )
    .map_err(storage_error)?;
    info.summary.revision = super::next(db, &info.summary.name)?;
    super::save(db, info, true)
}

pub(in crate::store) fn resolve(db: &Connection, info: &Info) -> Result<Resolved, Fault> {
    let saved = stored(db, info)?;
    if info.settings.is_none() {
        if info
            .extension
            .as_ref()
            .is_some_and(|extension| extension.settings_schema.is_some())
        {
            return Err(Fault::new(
                ErrorCode::NotConfigured,
                "plugin settings form is invalid",
            ));
        }
        return Ok(Resolved {
            values: BTreeMap::new(),
            secrets: BTreeMap::new(),
            authorizations: authorizations(&saved),
            mcp: saved.mcp,
        });
    }
    let state = project(db, info, &saved)?;
    if !state.ready {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin settings are incomplete or changed",
        ));
    }
    let secrets = state
        .configured
        .iter()
        .map(|name| {
            saved.slots[name]
                .clone()
                .map(|value| (name.clone(), value))
                .ok_or_else(|| invalid("configured plugin setting is unavailable"))
        })
        .collect::<Result<_, _>>()?;
    Ok(Resolved {
        values: state.values,
        secrets,
        authorizations: authorizations(&saved),
        mcp: saved.mcp,
    })
}

pub(in crate::store) fn capture(
    db: &Connection,
    packages: &[Info],
) -> BTreeMap<String, Result<Resolved, Fault>> {
    packages
        .iter()
        .map(|info| (info.summary.name.clone(), resolve(db, info)))
        .collect()
}

fn authorizations(saved: &Stored) -> BTreeMap<String, sailry_protocol::CredentialId> {
    saved
        .authorizations
        .iter()
        .filter(|(_, grant)| grant.credentials.is_some())
        .map(|(server, grant)| (server.clone(), grant.id))
        .collect()
}

pub(super) fn revisions(db: &Connection, plugin: &str) -> Result<Vec<(i64, Stored)>, Fault> {
    let mut query = db
        .prepare("SELECT revision,body FROM plugin_settings WHERE plugin=?1")
        .map_err(storage_error)?;
    query
        .query_map([plugin], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?
        .map(|row| {
            let (revision, body) = row.map_err(storage_error)?;
            Ok((
                revision,
                serde_json::from_slice(&body).map_err(storage_error)?,
            ))
        })
        .collect()
}

pub(super) fn update_revision(
    db: &Connection,
    plugin: &str,
    revision: i64,
    saved: &Stored,
) -> Result<(), Fault> {
    db.execute(
        "UPDATE plugin_settings SET body=?3 WHERE plugin=?1 AND revision=?2",
        params![plugin, revision, encode(saved)?],
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn edit_revisions(
    db: &Connection,
    plugin: &str,
    mut edit: impl FnMut(&mut Stored),
) -> Result<(), Fault> {
    for (revision, mut saved) in revisions(db, plugin)? {
        edit(&mut saved);
        update_revision(db, plugin, revision, &saved)?;
    }
    Ok(())
}

fn validate_values(schema: &Schema, values: &BTreeMap<String, Value>) -> Result<(), Fault> {
    if schema.properties.values().any(|field| {
        field
            .secret
            .as_ref()
            .is_some_and(|binding| binding.unsupported)
    }) {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "setting binding is not supported",
        ));
    }
    if encode(values)?.len() > MAX_BYTES {
        return Err(invalid("plugin settings exceed the size limit"));
    }
    for name in &schema.required {
        if schema.properties[name].secret.is_none() && !values.contains_key(name) {
            return Err(invalid("required plugin setting is missing"));
        }
    }
    for (name, value) in values {
        let field = schema
            .properties
            .get(name)
            .filter(|field| field.secret.is_none())
            .ok_or_else(|| invalid("unknown or secret field in public plugin settings"))?;
        validate_value(field, value)?;
    }
    Ok(())
}

fn validate_value(field: &Field, value: &Value) -> Result<(), Fault> {
    if value
        .as_str()
        .is_some_and(|value| value.chars().count() > MAX_STRING)
    {
        return Err(invalid("plugin setting exceeds the string limit"));
    }
    let schema = serde_json::to_value(field).map_err(storage_error)?;
    let validator = jsonschema::draft202012::options()
        .build(&schema)
        .map_err(|_| storage_error("invalid stored plugin settings schema"))?;
    if !validator.is_valid(value) {
        return Err(invalid("plugin setting violates its field constraints"));
    }
    Ok(())
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;
