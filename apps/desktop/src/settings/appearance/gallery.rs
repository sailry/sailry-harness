use super::*;

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
                let target = id.clone();
                let name = package.name.clone();
                v_flex().gap_1().min_w_0().child(
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
                                                ColorName::Orange.scale(if cx.theme().is_dark() {
                                                    500
                                                } else {
                                                    600
                                                })
                                            } else {
                                                cx.theme().muted_foreground
                                            }),
                                        ),
                                ),
                        )
                        .on_click(move |_, window, cx| menu::apply(&id, window, cx))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |workspace, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                workspace.theme_focus.focus(window, cx);
                                menu::show(&target, event.position, window, cx);
                            }),
                        ),
                )
            }))
    }
}
