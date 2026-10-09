use super::*;
use crate::{
    settings::{
        entry::Entry,
        group::{Group, Row},
    },
    tr,
};
use gpui_kit::component::{button::Button, select::Select, switch::Switch};
use sailry_protocol::Command;

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.saved.clone().unwrap_or_default();
        let locked = self.locked();
        let mut providers = Group::new("search_providers");
        for driver in Driver::ALL {
            let config = settings.providers.get(&driver).cloned().unwrap_or_default();
            let status = if !config.enabled {
                "search_disabled"
            } else if driver.key_required() && !config.key_configured {
                "search_key_required"
            } else if driver == Driver::Searxng
                && config.endpoint.as_ref().is_none_or(String::is_empty)
            {
                "search_endpoint_required"
            } else if driver == Driver::Exa && !config.key_configured {
                "search_free"
            } else {
                "search_configured"
            };
            let owner = cx.entity();
            providers = providers.child(
                Entry::new(
                    format!("search-driver-{}", id(driver)),
                    v_flex()
                        .min_w_0()
                        .gap_1()
                        .child(div().font_medium().child(tr(label(driver))))
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(tr(status)),
                        ),
                )
                .control(
                    div()
                        .debug_selector(move || format!("search-enabled-{}", id(driver)))
                        .child(
                            Switch::new(SharedString::from(format!(
                                "search-enabled-{}",
                                id(driver)
                            )))
                            .checked(config.enabled)
                            .disabled(locked)
                            .accessibility_label(tr(label(driver)))
                            .on_click(cx.listener(
                                move |panel, enabled, window, cx| {
                                    if let Some(mut settings) = panel.saved.clone() {
                                        settings.providers.entry(driver).or_default().enabled =
                                            *enabled;
                                        panel.save(settings, window, cx);
                                    }
                                },
                            )),
                        ),
                )
                .action(
                    Button::new(SharedString::from(format!("search-edit-{}", id(driver))))
                        .debug_selector(move || format!("search-edit-{}", id(driver)))
                        .icon(IconName::Settings2)
                        .tooltip(tr("settings_edit"))
                        .accessibility_label(tr("settings_edit"))
                        .disabled(locked)
                        .on_click(move |_, window, cx| {
                            _ = editor::open(owner.clone(), driver, window, cx);
                        }),
                )
                .action(
                    Button::new(SharedString::from(format!("search-test-{}", id(driver))))
                        .debug_selector(move || format!("search-test-{}", id(driver)))
                        .label(tr("search_test"))
                        .disabled(locked || self.binding.is_none())
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.submit(Command::TestSearchDriver { driver }, window, cx)
                        })),
                ),
            );
        }
        v_flex()
            .gap_6()
            .child(
                Group::new("search_settings")
                    .child(Row::new(
                        "search_prefer_native",
                        div()
                            .debug_selector(|| "search-prefer-native".into())
                            .child(
                                Switch::new("search-prefer-native")
                                    .checked(settings.prefer_native)
                                    .disabled(locked)
                                    .accessibility_label(tr("search_prefer_native"))
                                    .on_click(cx.listener(|panel, value, window, cx| {
                                        if let Some(mut settings) = panel.saved.clone() {
                                            settings.prefer_native = *value;
                                            panel.save(settings, window, cx);
                                        }
                                    })),
                            ),
                    ))
                    .child(
                        Row::new(
                            "search_default_driver",
                            div()
                                .debug_selector(|| "search-default-driver".into())
                                .w_full()
                                .min_w_0()
                                .child(
                                    Select::new(&self.picker)
                                        .w_full()
                                        .disabled(locked)
                                        .accessibility_label(tr("search_default_driver")),
                                ),
                        )
                        .wide(),
                    )
                    .child(Row::new(
                        "search_fallback",
                        div().debug_selector(|| "search-fallback".into()).child(
                            Switch::new("search-fallback")
                                .checked(settings.fallback)
                                .disabled(locked)
                                .accessibility_label(tr("search_fallback"))
                                .on_click(cx.listener(|panel, value, window, cx| {
                                    if let Some(mut settings) = panel.saved.clone() {
                                        settings.fallback = *value;
                                        panel.save(settings, window, cx);
                                    }
                                })),
                        ),
                    )),
            )
            .child(providers)
    }
}
