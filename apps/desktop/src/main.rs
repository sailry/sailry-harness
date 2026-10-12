// The explicit capture harness has no unit-test registrations; their helpers
// and unrelated application launch paths are intentionally unused in that build.
#![cfg_attr(feature = "design-capture", allow(dead_code, unused_imports))]

mod activity;
#[cfg(any(test, feature = "workload-tests"))]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/agent_support/mod.rs"]
mod agent_fixture;
#[cfg(target_os = "macos")]
mod app_menu;
#[cfg(test)]
use agent_fixture as agent_support;
mod application;
mod assets;
mod backend;
mod browser;
mod command_picker;
#[cfg(target_os = "macos")]
use sailry_node_runtime::computer_worker;
mod content;
mod conversation;
mod dictation;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/discovery_support/mod.rs"]
mod discovery_fixture;
mod downloads;
mod empty_state;
mod feedback;
mod header;
mod hosts;
mod live;
mod locale;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/support/mcp_input.rs"]
mod mcp_fixture;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/support/mcp.rs"]
mod mcp_peer;
mod model_picker;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/agent/providers/native/server.rs"]
mod native_provider_fixture;
mod navigation_tabs;
mod oauth;
mod pages;
mod panes;
mod permissions;
mod plugins;
mod ports;
mod preferences;
mod preview;
mod prompts;
#[cfg(test)]
#[allow(dead_code)]
mod provider_fixture;
mod reader;
mod reasoning;
mod resources;
mod settings;
mod shell;
mod shortcuts;
mod sidebar;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/ssh/server.rs"]
mod ssh_fixture;
mod startup;
mod system_shortcuts;
mod terminal;
mod theme;
mod ui;
mod updater;
#[cfg(feature = "workload-tests")]
mod workload;
mod workspace;

extern crate gpui_kit as gpui;
#[cfg(feature = "workload-tests")]
use application::run;
use application::window_options;
use gpui_kit::SharedString;

rust_i18n::i18n!("../../locales", fallback = "en");

fn tr(key: &str) -> SharedString {
    rust_i18n::t!(key).to_string().into()
}

fn main() {
    #[cfg(all(test, feature = "design-capture"))]
    {
        plugins::capture_design();
    }
    #[cfg(not(all(test, feature = "design-capture")))]
    application::launch();
}
