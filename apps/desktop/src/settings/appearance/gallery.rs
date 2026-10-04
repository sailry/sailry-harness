use super::*;
use gpui_kit::prelude::FluentBuilder as _;

impl Workspace {
    pub(super) fn theme_cards(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let catalog = cx.global::<theme::Catalog>();
        div()
            .grid()
            .grid_cols(3)
            .gap_3()
            .w_full()
            .debug_selector(|| "theme-gallery".into())
            .children(catalog.packages.iter().enumerate().map(|(index, package)| {
                let selected = catalog.selected == index;
                let id = package.id.clone();
                let name = package.name.clone();
                v_flex()
                    .gap_1()
                    .min_w_0()
                    .child(
                        Button::new(("theme-card", index))
                            .ghost()
                            .h_auto()
                            .w_full()
                            .p_0()
                            .debug_selector(move || format!("theme-card-{index}"))
                            .selected(selected)
                            .accessibility_label(name.clone())
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius_lg)
                            .overflow_hidden()
                            .child(
                                v_flex()
                                    .w_full()
                                    .min_w_0()
                                    .child(theme::preview(package, cx.theme().is_dark(), cx))
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .p_3()
                                            .gap_2()
                                            .bg(cx.theme().group_box)
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .text_sm()
                                                    .truncate()
                                                    .child(name.clone()),
                                            )
                                            .child(
                                                Icon::new(if selected {
                                                    IconName::CircleCheck
                                                } else {
                                                    IconName::Palette
                                                })
                                                .text_color(if selected {
                                                    cx.theme().foreground
                                                } else {
                                                    cx.theme().muted_foreground
                                                }),
                                            ),
                                    ),
                            )
                            .on_click(move |_, window, cx| theme::apply(index, Some(window), cx)),
                    )
                    .when(index > 0, |card| {
                        card.child(
                            Button::new(("theme-remove", index))
                                .ghost()
                                .small()
                                .ml_auto()
                                .icon(IconName::CircleX)
                                .debug_selector(move || format!("theme-remove-{index}"))
                                .tooltip(tr("settings_delete"))
                                .accessibility_label(tr("settings_delete"))
                                .on_click(move |_, window, cx| {
                                    let id = id.clone();
                                    let name = name.clone();
                                    crate::prompts::confirm(
                                        &tr("settings_delete"),
                                        &rust_i18n::t!("appearance_remove", name = name.as_ref()),
                                        tr("settings_delete"),
                                        window,
                                        cx,
                                        move |window, cx| {
                                            if let Err(error) = theme::remove(&id, window, cx) {
                                                eprintln!("could not remove theme: {error}");
                                                cx.update_global::<theme::Catalog, _>(
                                                    |catalog, _| {
                                                        catalog.error =
                                                            Some("appearance_remove_failed")
                                                    },
                                                );
                                            }
                                        },
                                    );
                                }),
                        )
                    })
            }))
    }
}
