//! Metadata comes from the same unmodified SDK as native execution.
use cua_driver_sdk::{CuaDriver, DriverHostOptions};
use serde_json::Value;
use std::sync::OnceLock;

pub(crate) fn catalog() -> &'static Value {
    static TOOLS: OnceLock<Value> = OnceLock::new();
    TOOLS.get_or_init(|| CuaDriver::inspect_host_tools(options(None))["tools"].clone())
}

pub(super) fn options(host_bundle_id: Option<String>) -> DriverHostOptions {
    DriverHostOptions {
        cursor: Default::default(),
        host_owns_permission_ux: true,
        host_bundle_id,
        claude_code_compatibility: false,
        prepare_desktop_environment: true,
        register_host_tools: None,
        authorization_host: None,
        activity_observer: None,
    }
}

pub(crate) fn definition(name: &str) -> Option<&'static Value> {
    catalog()
        .as_array()?
        .iter()
        .find(|tool| tool["name"] == name)
}

pub(crate) fn read_only(name: &str) -> Option<bool> {
    definition(name).map(|tool| {
        tool["annotations"]["readOnlyHint"]
            .as_bool()
            .unwrap_or(false)
    })
}
