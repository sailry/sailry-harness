//! Node-owned extensions rendered in Desktop with the pinned Kit bindings.
mod activity;
mod artifact;
mod browser;
mod cache;
mod charts;
mod composer;
pub(crate) mod contributions;
mod controls;
mod conversation;
mod credentials;
mod data_table;
mod diff;
pub(crate) mod documents;
pub(crate) mod emblem;
pub(crate) mod file_transfers;
mod files;
#[cfg(test)]
pub(crate) mod fixture;
mod forms;
#[cfg(test)]
mod game_fixture;
mod header;
mod host;
pub(crate) use host::usage;
mod images;
mod layout;
mod location;
mod menu;
pub(crate) mod metadata;
mod native_context;
pub(crate) mod navigation;
mod notifications;
mod overlay;
mod panel;
pub(crate) mod panes;
mod picker;
pub(crate) mod preload;
mod previews;
pub(crate) mod projects;
mod references;
pub(crate) mod resource;
mod runtime;
mod ssh_transfers;
mod terminals;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn capture_design() {
    tests::task_plaza::rendering::run();
}
mod tree;
#[cfg(test)]
pub(crate) use tests::diagnostics;
mod theme;
mod view;
mod window;
pub(crate) mod workspace;

pub(crate) use panel::{ConversationEvent, Panel};

pub(crate) fn init(cx: &mut gpui_kit::App) {
    gpui_shell::init(cx);
    contributions::native::init(cx);
    native_context::init(cx);
}

#[cfg(all(feature = "workload-tests", target_os = "macos"))]
pub(crate) fn verify_document(output: &std::path::Path) {
    artifact::native_check::run(output);
}

#[cfg(all(feature = "workload-tests", target_os = "macos"))]
pub(crate) async fn prepare_document(
    root: &std::path::Path,
) -> (
    sailry_node_runtime::Node,
    std::sync::Arc<crate::agent_fixture::Server>,
    [(sailry_protocol::NodeId, sailry_protocol::Session); 2],
) {
    artifact::native_flow::prepare(root).await
}

#[cfg(all(feature = "workload-tests", target_os = "macos"))]
pub(crate) async fn verify_document_flow(
    output: &std::path::Path,
    sessions: [(sailry_protocol::NodeId, sailry_protocol::Session); 2],
    server: std::sync::Arc<crate::agent_fixture::Server>,
    shell: gpui_kit::Entity<crate::shell::Shell>,
    cx: &mut gpui_kit::AsyncWindowContext,
) {
    artifact::native_flow::verify(output, sessions, server, shell, cx).await;
}
