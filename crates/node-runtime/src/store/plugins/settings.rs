//! Immutable configuration revisions share plugin inventory transactions and receipts.
use super::*;
pub(in crate::store) mod history;
pub(super) mod targets;
use crate::plugins::settings::Resolved;
use sailry_protocol::{Secret, plugin::settings::*};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::store) struct Stored {
    pub(super) schema: Option<Schema>,
    #[serde(default)]
    pub(super) values: BTreeMap<String, Value>,
    #[serde(default)]
    pub(super) slots: BTreeMap<String, Option<Secret>>,
    #[serde(default)]
    pub(super) mcp_targets: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) authorizations: BTreeMap<String, history::McpGrant>,
    #[serde(default)]
    pub(super) mcp: Option<plugin::mcp::Configuration>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) mcp_allowed: BTreeMap<String, bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) github: Option<history::Identity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) oauth: Option<history::Authorized>,
}

pub(super) async fn current(db: &Connection, package: &plugin::Reference) -> Result<Info, Fault> {
    let info = required(db, &package.name).await?;
    if info.summary.reference() != *package {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin configuration changed",
        ));
    }
    Ok(info)
}

pub(super) async fn stored(db: &Connection, info: &Info) -> Result<Stored, Fault> {
    if info.summary.settings_revision == 0 {
        return Ok(Stored {
            schema: info.settings.clone(),
            values: defaults(info.settings.as_ref()),
            slots: BTreeMap::new(),
            mcp_targets: BTreeMap::new(),
            authorizations: BTreeMap::new(),
            mcp: None,
            mcp_allowed: BTreeMap::new(),
            github: None,
            oauth: None,
        });
    }
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM plugin_settings WHERE plugin=?1 AND revision=?2")
            .await
            .map_err(storage_error)?;
        match statement
            .query_row(params![
                info.summary.name.as_str(),
                info.summary.settings_revision as i64
            ])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    serde_json::from_slice(&body.ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "plugin configuration revision is unavailable",
        )
    })?)
    .map_err(storage_error)
}

pub(in crate::store) async fn validate_revision(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<(), Fault> {
    if package.settings_revision == 0 {
        return Ok(());
    }
    if package.settings_revision > i64::MAX as u64 {
        return Err(sailry_protocol::Fault::invalid(
            "invalid plugin configuration revision",
        ));
    }
    let exists: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM plugin_settings WHERE plugin=?1 AND revision=?2)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![
                package.name.as_str(),
                package.settings_revision as i64
            ])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
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

fn same_binding(
    previous: &Stored,
    schema: &Schema,
    name: &str,
    targets: &BTreeMap<String, String>,
) -> bool {
    let destination = previous.mcp_targets.get(name);
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
        if !previous.unsupported && !current.unsupported && previous == current
            && (current.server.is_none() || destination.is_some() && destination == targets.get(name)))
}

fn can_keep(
    stored: &Stored,
    schema: &Schema,
    name: &str,
    targets: &BTreeMap<String, String>,
) -> bool {
    if same_binding(stored, schema, name, targets) {
        return true;
    }
    !stored.slots.get(name).is_some_and(Option::is_some)
}

async fn project(db: &Connection, info: &Info, stored: &Stored) -> Result<State, Fault> {
    // Missing legacy targets remain stored but cannot authorize a new destination.
    let resolved = targets::current(db, info, stored.mcp.as_ref()).await;
    let targets_available = resolved.is_ok();
    let targets = resolved.unwrap_or_default();
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
        if same_binding(stored, schema, name, &targets) && value.is_some() {
            configured.push(name.clone());
        }
    }
    let ready = targets_available
        && stored.schema.as_ref() == Some(schema)
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
            .filter(|(name, field)| {
                field.secret.is_some() && can_keep(stored, schema, name, &targets)
            })
            .map(|(name, _)| name.clone())
            .collect(),
        ready,
    })
}

pub(in crate::store) async fn read(
    db: &Connection,
    package: &plugin::Reference,
) -> Result<State, Fault> {
    let info = current(db, package).await?;
    project(db, &info, &stored(db, &info).await?).await
}

pub(in crate::store) async fn read_secret(
    db: &Connection,
    package: &plugin::Reference,
    field: &str,
) -> Result<Option<sailry_protocol::Secret>, Fault> {
    let info = current(db, package).await?;
    let saved = stored(db, &info).await?;
    let schema = info
        .settings
        .as_ref()
        .ok_or_else(|| sailry_protocol::Fault::invalid("plugin has no settings form"))?;
    if !schema
        .properties
        .get(field)
        .is_some_and(|field| field.secret.is_some())
    {
        return Err(sailry_protocol::Fault::invalid(
            "setting is not a declared secret slot",
        ));
    }
    let targets = targets::current(db, &info, saved.mcp.as_ref()).await?;
    if !same_binding(&saved, schema, field, &targets) {
        return Err(sailry_protocol::Fault::invalid("setting binding changed"));
    }
    Ok(saved.slots.get(field).cloned().flatten())
}

pub(in crate::store) async fn read_admitted(db: &Connection, info: &Info) -> Result<State, Fault> {
    project(db, info, &stored(db, info).await?).await
}

pub(in crate::store) async fn save(
    db: &Connection,
    package: &plugin::Reference,
    values: &BTreeMap<String, Value>,
    updates: &BTreeMap<String, SecretUpdate>,
) -> Result<State, Fault> {
    let mut info = current(db, package).await?;
    let schema = info.settings.as_ref().ok_or_else(|| {
        Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no valid settings form",
        )
    })?;
    let previous = stored(db, &info).await?;
    let targets = match targets::current(db, &info, previous.mcp.as_ref()).await {
        Ok(targets) => targets,
        Err(_)
            if updates
                .values()
                .all(|update| matches!(update, SecretUpdate::Clear)) =>
        {
            BTreeMap::new()
        }
        Err(error) => return Err(error),
    };
    validate_values(schema, values)?;
    let models = if schema.properties.values().any(|field| field.model) {
        super::models::catalog(db).await?.models
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
            let selected = values.get(model).and_then(Value::as_str).ok_or_else(|| {
                sailry_protocol::Fault::invalid("reasoning requires a selected model")
            })?;
            let model = models
                .iter()
                .find(|model| model.id == selected)
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("reasoning requires an available model")
                })?;
            if !model
                .efforts
                .iter()
                .any(|effort| plugin::models::effort_key(*effort) == value)
            {
                return Err(sailry_protocol::Fault::invalid(
                    "reasoning choice is not configured for this model",
                ));
            }
        }
    }
    let size = serde_json::to_vec(&(values, updates))
        .map_err(storage_error)?
        .len();
    if size > MAX_BYTES {
        return Err(sailry_protocol::Fault::invalid(
            "plugin settings exceed the size limit",
        ));
    }
    let fields: BTreeMap<_, _> = schema
        .properties
        .iter()
        .filter(|(_, field)| field.secret.is_some())
        .collect();
    if updates.len() != fields.len() || updates.keys().any(|name| !fields.contains_key(name)) {
        return Err(sailry_protocol::Fault::invalid(
            "each secret setting requires an explicit update intent",
        ));
    }
    for (name, update) in updates {
        match update {
            SecretUpdate::Replace(value) => {
                if value.expose().is_empty() || value.expose().contains('\0') {
                    return Err(sailry_protocol::Fault::invalid(
                        "secret replacement must be nonempty and contain no NUL",
                    ));
                }
                validate_value(fields[name], &Value::String(value.expose().into()))?;
                if fields[name]
                    .secret
                    .as_ref()
                    .is_some_and(|binding| binding.header.is_some())
                {
                    reqwest::header::HeaderValue::from_str(value.expose()).map_err(|_| {
                        sailry_protocol::Fault::invalid("invalid secret header value")
                    })?;
                }
            }
            SecretUpdate::Keep if !can_keep(&previous, schema, name, &targets) => {
                return Err(sailry_protocol::Fault::invalid(
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
                })
                .await?;
                slots.insert(name.clone(), None);
            }
        }
    }
    let saved = Stored {
        schema: Some(schema.clone()),
        values: values.clone(),
        slots,
        mcp_targets: targets,
        authorizations: previous.authorizations,
        mcp: previous.mcp,
        mcp_allowed: previous.mcp_allowed,
        github: previous.github,
        oauth: previous.oauth,
    };
    persist(db, &mut info, &saved).await?;
    super::super::dispatch::settings_changed(db, &info.summary.reference()).await?;
    project(db, &info, &saved).await
}

pub(super) async fn persist(db: &Connection, info: &mut Info, saved: &Stored) -> Result<(), Fault> {
    let next: i64 = {
        let mut statement = db
            .prepare("SELECT settings_revision FROM plugins WHERE name=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([info.summary.name.as_str()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    info.summary.settings_revision = next
        .checked_add(1)
        .ok_or_else(|| sailry_protocol::Fault::invalid("plugin configuration revision exhausted"))?
        as u64;
    db.execute(
        "INSERT INTO plugin_settings(plugin,revision,body) VALUES(?1,?2,?3)",
        params![
            info.summary.name.as_str(),
            info.summary.settings_revision as i64,
            encode(saved)?
        ],
    )
    .await
    .map_err(storage_error)?;
    // The inventory tombstone retains this counter when unused revisions are collected.
    db.execute(
        "UPDATE plugins SET settings_revision=?2 WHERE name=?1",
        params![
            info.summary.name.as_str(),
            info.summary.settings_revision as i64
        ],
    )
    .await
    .map_err(storage_error)?;
    info.summary.revision = super::next(db, &info.summary.name).await?;
    super::save(db, info, true).await
}

pub(in crate::store) async fn resolve(db: &Connection, info: &Info) -> Result<Resolved, Fault> {
    let saved = stored(db, info).await?;
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
            mcp: saved.mcp,
            mcp_allowed: saved.mcp_allowed,
        });
    }
    let state = project(db, info, &saved).await?;
    if !state.ready {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin settings are incomplete or changed",
        ));
    }
    authorize_slots(db, info, &saved, &state.configured).await?;
    let secrets = state
        .configured
        .iter()
        .map(|name| {
            saved
                .slots
                .get(name)
                .cloned()
                .flatten()
                .map(|value| (name.clone(), value))
                .ok_or_else(|| {
                    sailry_protocol::Fault::invalid("configured plugin setting is unavailable")
                })
        })
        .collect::<Result<_, _>>()?;
    Ok(Resolved {
        values: state.values,
        secrets,
        mcp: saved.mcp,
        mcp_allowed: saved.mcp_allowed,
    })
}

async fn authorize_slots(
    db: &Connection,
    info: &Info,
    saved: &Stored,
    configured: &[String],
) -> Result<(), Fault> {
    if configured.is_empty() {
        return Ok(());
    }
    // Tombstones retain the latest declaration after removal. Historical bytes
    // remain replayable, but cannot restore a slot retired into a connector.
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM plugins WHERE name=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([info.summary.name.as_str()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    let current: Option<Info> = body
        .map(|body| serde_json::from_slice(&body).map_err(storage_error))
        .transpose()?;
    let Some(current) =
        current.filter(|current| !super::connectors::declarations(current).is_empty())
    else {
        return Ok(());
    };
    // The projection already validated this capture's immutable destination.
    if let Some(schema) = &current.settings
        && configured
            .iter()
            .all(|name| same_binding(saved, schema, name, &saved.mcp_targets))
    {
        return Ok(());
    }
    Err(Fault::new(
        ErrorCode::NotConfigured,
        "plugin credential binding is no longer configured",
    ))
}

pub(in crate::store) async fn capture(
    db: &Connection,
    packages: &[Info],
) -> BTreeMap<String, Result<Resolved, Fault>> {
    let mut selected = BTreeMap::new();
    for info in packages {
        selected.insert(info.summary.name.clone(), resolve(db, info).await);
    }
    selected
}

pub(super) async fn revisions(db: &Connection, plugin: &str) -> Result<Vec<(i64, Stored)>, Fault> {
    let mut query = db
        .prepare("SELECT revision,body FROM plugin_settings WHERE plugin=?1")
        .await
        .map_err(storage_error)?;
    (async {
        let mut rows = query.query([plugin]).await?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await? {
            let mapped: turso::Result<_> =
                (|row: &turso::Row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?)))(&row);
            values.push(mapped);
        }
        Ok::<_, turso::Error>(values.into_iter())
    })
    .await
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

pub(super) async fn update_revision(
    db: &Connection,
    plugin: &str,
    revision: i64,
    saved: &Stored,
) -> Result<(), Fault> {
    db.execute(
        "UPDATE plugin_settings SET body=?3 WHERE plugin=?1 AND revision=?2",
        params![plugin, revision, encode(saved)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn edit_revisions(
    db: &Connection,
    plugin: &str,
    mut edit: impl FnMut(&mut Stored),
) -> Result<(), Fault> {
    for (revision, mut saved) in revisions(db, plugin).await? {
        edit(&mut saved);
        update_revision(db, plugin, revision, &saved).await?;
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
        return Err(sailry_protocol::Fault::invalid(
            "plugin settings exceed the size limit",
        ));
    }
    for name in &schema.required {
        if schema.properties[name].secret.is_none() && !values.contains_key(name) {
            return Err(sailry_protocol::Fault::invalid(
                "required plugin setting is missing",
            ));
        }
    }
    for (name, value) in values {
        let field = schema
            .properties
            .get(name)
            .filter(|field| field.secret.is_none())
            .ok_or_else(|| {
                sailry_protocol::Fault::invalid("unknown or secret field in public plugin settings")
            })?;
        validate_value(field, value)?;
    }
    Ok(())
}

fn validate_value(field: &Field, value: &Value) -> Result<(), Fault> {
    if field.time
        && value
            .as_str()
            .and_then(sailry_protocol::plugin::settings::parse_time)
            .is_none()
    {
        return Err(Fault::invalid("plugin time setting must use HH:mm"));
    }
    if field.child_concurrency {
        crate::plugins::settings::child_limit(value)?;
    }
    if value
        .as_str()
        .is_some_and(|value| value.chars().count() > MAX_STRING)
    {
        return Err(sailry_protocol::Fault::invalid(
            "plugin setting exceeds the string limit",
        ));
    }
    let schema = serde_json::to_value(field).map_err(storage_error)?;
    let validator = jsonschema::draft202012::options()
        .build(&schema)
        .map_err(|_| storage_error("invalid stored plugin settings schema"))?;
    if !validator.is_valid(value) {
        return Err(sailry_protocol::Fault::invalid(
            "plugin setting violates its field constraints",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;
