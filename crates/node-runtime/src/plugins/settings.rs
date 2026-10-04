//! Settings semantics adapted from harbor-agent-plugins 727ce0a settings.rs
//! (Apache-2.0). JSON Schema validation uses the pinned library, not another evaluator.
mod validation;

use super::{invalid, mcp, package};
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
}

pub(super) fn discover(
    directory: &Dir,
    extension: Option<&Extension>,
    servers: &std::collections::BTreeMap<String, mcp::Server>,
    issues: &mut Vec<Issue>,
) -> Option<Schema> {
    let path = extension?.settings_schema.as_ref()?;
    let result = (|| {
        let path = path.strip_prefix("./").unwrap_or(path);
        let parts = crate::files::path::components(path, false)?;
        if path.len() > 512 || !path.ends_with(".json") {
            return Err(invalid("settings schema must be a package JSON resource"));
        }
        let (file, parents) = parts
            .split_last()
            .ok_or_else(|| invalid("empty settings path"))?;
        let parent =
            crate::files::path::descend(directory.try_clone().map_err(super::io_error)?, parents)?;
        let bytes = package::read(&parent, file, MAX_BYTES)?
            .ok_or_else(|| invalid("settings schema is unavailable"))?;
        let schema = validation::parse(&bytes, servers)?;
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

#[cfg(test)]
mod tests;
