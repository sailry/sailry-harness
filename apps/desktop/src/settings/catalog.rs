use super::{
    Section, Workspace,
    group::{Group, Row},
};
use crate::tr;
use gpui_kit::component::v_flex;
use gpui_kit::*;

impl Workspace {
    pub(super) fn catalog(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            Section::Connections => Group::new("settings_pairing")
                .child(
                    Row::new("connections_pair", div()).description("connections_pair_description"),
                )
                .child(
                    Row::new("connections_host", div()).description("connections_host_description"),
                )
                .into_any_element(),
            Section::About => v_flex()
                .gap_4()
                .child(
                    Group::new("app")
                        .child(Row::new("settings_version", env!("CARGO_PKG_VERSION")))
                        .child(Row::new(
                            "settings_build",
                            tr(if cx.try_global::<crate::backend::Services>().is_none() {
                                "preview"
                            } else if cfg!(debug_assertions) {
                                "settings_build_debug"
                            } else {
                                "settings_build_release"
                            }),
                        ))
                        .child(Row::new("settings_license", tr("settings_license_pending"))),
                )
                .child(self.updater.clone())
                .into_any_element(),
            _ => unreachable!(),
        }
    }
}
