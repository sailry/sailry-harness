//! Plugin configuration uses declared settings panels or the shared schema dialog.
use super::*;
use crate::settings::{plugins::configuration, providers::Binding};
use gpui_kit::component::notification;

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry, no_json)]
pub(crate) struct OpenPanel {
    pub node: sailry_protocol::NodeId,
    pub package: Reference,
}
impl EventEmitter<OpenPanel> for Workspace {}

pub(crate) fn has_panel(info: &Info) -> bool {
    info.extension
        .as_ref()
        .and_then(|extension| extension.settings_page.as_ref())
        .is_some()
}

pub(crate) fn configurable(info: &Info) -> bool {
    has_panel(info)
        || info
            .settings
            .as_ref()
            .is_some_and(|schema| !schema.properties.is_empty())
}

pub(in crate::settings) fn open(
    owner: Entity<Workspace>,
    binding: Binding,
    info: Info,
    window: &mut Window,
    cx: &mut App,
) {
    if has_panel(&info) {
        window.dispatch_action(
            Box::new(OpenPanel {
                node: binding.client.target(),
                package: info.summary.reference(),
            }),
            cx,
        );
    } else if configurable(&info) {
        let editor = configuration::panel(binding, info.summary, window, cx);
        configuration::bind_owner(&editor, &owner, window, cx);
    } else {
        crate::feedback::toast(
            window,
            tr("plugins_view_unavailable"),
            notification::Notification::error(tr("plugins_view_unavailable")),
            cx,
        );
    }
}
