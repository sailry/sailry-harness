//! Plugin configuration uses declared settings panels or the shared schema dialog.
use super::*;
use crate::settings::{plugins::configuration, providers::Binding};

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenPanel {
    pub node: sailry_protocol::NodeId,
    pub name: String,
}
impl EventEmitter<OpenPanel> for Workspace {}

pub(crate) fn has_panel(info: &Info) -> bool {
    info.extension
        .as_ref()
        .and_then(|extension| extension.settings_page.as_ref())
        .is_some()
}

pub(crate) fn configurable(info: &Info) -> bool {
    info.settings
        .as_ref()
        .is_some_and(|schema| !schema.properties.is_empty())
        || info
            .extension
            .as_ref()
            .and_then(|extension| extension.settings_page.as_ref())
            .is_some_and(|page| page.entry.is_some())
}

pub(in crate::settings) fn open(binding: Binding, info: Info, window: &mut Window, cx: &mut App) {
    if has_panel(&info) {
        window.dispatch_action(
            Box::new(OpenPanel {
                node: binding.client.target(),
                name: info.summary.name,
            }),
            cx,
        );
    } else if configurable(&info) {
        configuration::panel(binding, info.summary, window, cx);
    }
}
