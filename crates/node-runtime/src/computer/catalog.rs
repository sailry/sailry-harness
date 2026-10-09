//! Metadata comes from the same unmodified SDK as native execution.
//! Sailry exposes a reviewed subset; new SDK tools stay hidden until reviewed.
use cua_driver_sdk::{CuaDriver, DriverHostOptions};
use serde_json::Value;
use std::sync::OnceLock;

/// Desktop observation and input tools. Browser automation, recording,
/// lifecycle, diagnostics, configuration and cursor styling stay internal:
/// Sailry binds the SDK session itself and owns browser and permission flows.
pub const TOOLS: &[&str] = &[
    "list_apps",
    "list_windows",
    "get_window_state",
    "verify_state",
    "zoom",
    "get_desktop_state",
    "get_screen_size",
    "launch_app",
    "bring_to_front",
    "set_window_frame",
    "invoke_menu",
    "click",
    "double_click",
    "right_click",
    "drag",
    "type_text",
    "press_key",
    "hotkey",
    "set_value",
    "scroll",
    "move_cursor",
    "clipboard_read",
    "clipboard_write",
    "kill_app",
];

/// Native parameters that read or write arbitrary host paths outside worktree
/// confinement. The trusted SDK handle binds `session` itself.
pub const HIDDEN: &[&str] = &[
    "session",
    "screenshot_out_file",
    "debug_image_out",
    "file_path",
    "image_path",
];

/// Reads the SDK marks read-only that can expose secrets, such as copied
/// passwords, still require the user's approval.
const PRIVATE: &[&str] = &["clipboard_read"];

pub(crate) fn catalog() -> &'static Value {
    static EXPOSED: OnceLock<Value> = OnceLock::new();
    EXPOSED.get_or_init(|| {
        let mut tools = CuaDriver::inspect_host_tools(options(None))["tools"].clone();
        if let Some(tools) = tools.as_array_mut() {
            tools.retain(|tool| {
                tool["name"]
                    .as_str()
                    .is_some_and(|name| TOOLS.contains(&name))
            });
        }
        tools
    })
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
        !PRIVATE.contains(&name)
            && tool["annotations"]["readOnlyHint"]
                .as_bool()
                .unwrap_or(false)
    })
}
