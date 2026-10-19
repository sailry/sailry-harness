//! Node-owned extensions rendered in Desktop with the pinned Kit bindings.
mod activity;
mod artifact;
mod browser;
mod cache;
pub(crate) mod cards;
mod charts;
mod composer;
pub(crate) mod contributions;
mod controls;
mod conversation;
mod credentials;
mod data_table;
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
mod theme;
mod tree;
mod view;
mod window;
pub(crate) mod workspace;

pub(crate) use panel::{ConversationEvent, Panel};

pub(crate) fn init(cx: &mut gpui_kit::App) {
    // Kit already owns Base initialization; Shell's reflection table initializes lazily.
    // Initializing Shell again would duplicate native input key bindings.
    contributions::native::init(cx);
    native_context::init(cx);
}
