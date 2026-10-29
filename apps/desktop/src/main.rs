mod activity;
#[cfg(any(test, feature = "workload-tests"))]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/agent_support/mod.rs"]
mod agent_fixture;
#[cfg(target_os = "macos")]
mod app_menu;
mod application;
mod assets;
mod backend;
mod browser;
mod command_picker;
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
mod file_picker;
mod header;
mod hosts;
mod installations;
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
mod onboarding;
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

fn main() -> std::process::ExitCode {
    application::launch()
}
