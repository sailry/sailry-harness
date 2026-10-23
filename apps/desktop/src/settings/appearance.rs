mod export;
mod gallery;
mod import;
mod menu;
pub(super) mod opacity;
pub(in crate::settings) mod updates;
use super::{
    Workspace,
    group::{Group, Row},
};
use crate::{theme, tr};
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    switch::Switch,
    tab::{Tab, TabBar},
    *,
};
use gpui_kit::*;

impl Workspace {
    pub(in crate::settings) fn open_theme(&self, id: &str, window: &mut Window, cx: &mut App) {
        if let Some(package) = cx
            .global::<theme::Catalog>()
            .packages
            .iter()
            .find(|package| package.id == id)
            .cloned()
        {
            import::details(package, self.theme_updates.clone(), window, cx);
        }
    }

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
            .track_focus(&self.theme_focus)
            .on_action(cx.listener(Self::theme_action))
            .gap_6()
            .child(
                Group::new("settings_interface")
                    .child(Row::new(
                        "appearance_mode",
                        TabBar::new("appearance-mode")
                            .segmented()
                            .selected_index(selected)
                            .children(
                                ["appearance_system", "appearance_light", "appearance_dark"].map(
                                    |key| {
                                        Tab::new().debug_selector(move || key.into()).label(tr(key))
                                    },
                                ),
                            )
                            .on_click(|index, window, cx| {
                                theme::select(
                                    [None, Some(ThemeMode::Light), Some(ThemeMode::Dark)][*index],
                                    window,
                                    cx,
                                )
                            }),
                    ))
                    .child(Row::new(
                        "appearance_effects",
                        div().debug_selector(|| "appearance-effects".into()).child(
                            Switch::new("appearance-effects")
                                .accessibility_label(tr("appearance_effects"))
                                .checked(crate::preferences::data(cx).theme_effects.unwrap_or(true))
                                .on_click(|checked, _, cx| {
                                    crate::preferences::update(cx, |data| {
                                        data.theme_effects = Some(*checked)
                                    });
                                    cx.refresh_windows();
                                }),
                        ),
                    )),
            )
            .child(self.opacity.controls(cx))
            .child(
                Group::new("appearance_packages")
                    .card(false)
                    .child(self.theme_cards(cx))
                    .action(
                        Button::new("themes-check-updates")
                            .label(tr("plugins_check_updates"))
                            .loading(self.theme_updates.read(cx).checking())
                            .disabled(
                                self.theme_updates.read(cx).service.is_none()
                                    || self.theme_updates.read(cx).checking(),
                            )
                            .on_click({
                                let updates = self.theme_updates.clone();
                                move |_, window, cx| {
                                    updates.update(cx, |state, cx| state.check(window, cx));
                                }
                            }),
                    )
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
