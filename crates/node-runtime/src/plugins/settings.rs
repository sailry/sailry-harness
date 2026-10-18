//! Settings semantics adapted from harbor-agent-plugins 727ce0a settings.rs
//! (Apache-2.0). JSON Schema validation uses the pinned library, not another evaluator.
mod validation;

use super::{mcp, package};
use cap_std::fs::Dir;
use sailry_protocol::{
    Fault,
    plugin::{Extension, Issue, IssueKind, settings::*},
};

/// Private execution inputs; never serialized into protocol values or events.
#[derive(Clone)]
pub(crate) struct Resolved {
    pub(crate) values: std::collections::BTreeMap<String, serde_json::Value>,
    pub(crate) secrets: std::collections::BTreeMap<String, sailry_protocol::Secret>,
    pub(crate) authorizations: std::collections::BTreeMap<String, sailry_protocol::CredentialId>,
    pub(crate) mcp: Option<sailry_protocol::plugin::mcp::Configuration>,
    pub(crate) github: Option<super::github::login::Identity>,
}

pub(super) fn discover(
    directory: &Dir,
    extension: Option<&Extension>,
    servers: &std::collections::BTreeMap<String, mcp::Server>,
    issues: &mut Vec<Issue>,
) -> Option<Schema> {
    let extension = extension?;
    let path = extension.settings_schema.as_ref()?;
    let result = (|| {
        let path = path.strip_prefix("./").unwrap_or(path);
        let parts = crate::files::path::components(path, false)?;
        if path.len() > 512 || !path.ends_with(".json") {
            return Err(sailry_protocol::Fault::invalid(
                "settings schema must be a package JSON resource",
            ));
        }
        let (file, parents) = parts
            .split_last()
            .ok_or_else(|| sailry_protocol::Fault::invalid("empty settings path"))?;
        let parent =
            crate::files::path::descend(directory.try_clone().map_err(super::io_error)?, parents)?;
        let bytes = package::read(&parent, file, MAX_BYTES)?
            .ok_or_else(|| sailry_protocol::Fault::invalid("settings schema is unavailable"))?;
        let schema = validation::parse(&bytes, servers)?;
        if schema
            .properties
            .values()
            .any(|field| field.child_concurrency)
            && (extension.scope != sailry_protocol::plugin::Scope::Host
                || !extension
                    .actions
                    .contains(&sailry_protocol::plugin::Action::DelegateAgents))
        {
            return Err(Fault::invalid(
                "child concurrency requires a host delegation declaration",
            ));
        }
        Ok(schema)
    })();
    match result {
        Ok(schema) => Some(schema),
        Err(_) => {
            issues.push(Issue {
                path: path.clone(),
                kind: IssueKind::InvalidExtensions,
            });
            None
        }
    }
}

pub(crate) fn child_limit(value: &serde_json::Value) -> Result<u64, Fault> {
    value
        .as_str()
        .filter(|text| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| Fault::invalid("child concurrency must be a positive decimal integer"))
}

pub(super) fn option_schema(value: &serde_json::Value) -> Result<Schema, Fault> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| Fault::invalid("invalid conversation option schema"))?;
    let schema = validation::parse(&bytes, &Default::default())?;
    if schema.properties.values().any(|field| {
        field.secret.is_some()
            || field.model
            || field.effort.is_some()
            || field.child_concurrency
            || field
                .import
                .as_ref()
                .is_some_and(|name| !sailry_protocol::plugin::ui::identifier(name))
    }) {
        return Err(Fault::invalid(
            "conversation options must be public scalar values",
        ));
    }
    let imports = schema
        .properties
        .values()
        .filter_map(|field| field.import.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    if imports.len()
        != schema
            .properties
            .values()
            .filter(|field| field.import.is_some())
            .count()
    {
        return Err(Fault::invalid(
            "conversation option imports must be distinct",
        ));
    }
    Ok(schema)
}

pub(super) fn validate_options(
    schema: &Schema,
    values: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<(), Fault> {
    let value =
        serde_json::to_value(values).map_err(|_| Fault::invalid("invalid conversation options"))?;
    if serde_json::to_vec(&value)
        .map_err(|_| Fault::invalid("invalid conversation options"))?
        .len()
        > MAX_BYTES
        || !validation::build(
            &serde_json::to_value(schema)
                .map_err(|_| Fault::invalid("invalid conversation option schema"))?,
        )?
        .is_valid(&value)
    {
        return Err(Fault::invalid(
            "conversation options do not match their declared schema",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
