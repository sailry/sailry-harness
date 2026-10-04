mod gallery;
mod import;
pub(super) mod opacity;
#[cfg(test)]
mod tests;
use super::{
    Workspace,
    group::{Group, Row},
};
use crate::{theme, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    tab::{Tab, TabBar},
    *,
};
use gpui_kit::*;

impl Workspace {
    pub(super) fn appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = if theme::follows_system(cx) {
            0
        } else if cx.theme().is_dark() {
            2
        } else {
            1
        };
        let catalog = cx.global::<theme::Catalog>();
        let incomplete = catalog.packages[catalog.selected].incomplete;
        v_flex()
            .gap_6()
            .child(
                Group::new("settings_interface").child(Row::new(
                    "appearance_mode",
                    TabBar::new("appearance-mode")
                        .segmented()
                        .selected_index(selected)
                        .children(
                            ["appearance_system", "appearance_light", "appearance_dark"].map(
                                |key| Tab::new().debug_selector(move || key.into()).label(tr(key)),
                            ),
                        )
                        .on_click(|index, window, cx| {
                            theme::select(
                                [None, Some(ThemeMode::Light), Some(ThemeMode::Dark)][*index],
                                window,
                                cx,
                            )
                        }),
                )),
            )
            .child(self.opacity.controls(cx))
            .child(
                Group::new("appearance_packages")
                    .card(false)
                    .child(self.theme_cards(cx))
                    .action(
                        Button::new("theme-import")
                            .primary()
                            .debug_selector(|| "theme-import".into())
                            .label(tr("appearance_import"))
                            .on_click(|_, window, cx| import::choose(window, cx)),
                    ),
            )
            .children(incomplete.then(|| {
                div()
                    .text_color(cx.theme().warning)
                    .child(tr("appearance_resource_missing"))
            }))
            .into_any_element()
    }
}
