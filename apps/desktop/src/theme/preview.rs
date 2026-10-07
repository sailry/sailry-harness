use super::{Package, decoration::asset, raster};
use gpui_kit::{component::*, *};

pub fn preview(package: &Package, dark: bool, cx: &App) -> AnyElement {
    let mut theme = cx.theme().clone();
    theme.apply_config(if dark { &package.dark } else { &package.light });
    // Gallery and import cards share a one-pixel Kit border.
    let radius = (cx.theme().radius_lg - px(1.)).max(px(0.));
    div()
        .id(format!("theme-preview-{}-{dark}", package.id))
        .debug_selector({
            let id = package.id.clone();
            move || format!("theme-preview-surface-{id}-{dark}")
        })
        .relative()
        .w_full()
        .h_24()
        .rounded_t(radius)
        .overflow_hidden()
        .bg(theme.background)
        .children(
            asset(package, "background.window", dark)
                .or_else(|| asset(package, "background.content", dark))
                .map(|(decoration, image)| {
                    div().absolute().inset_0().child(raster::rounded(
                        decoration,
                        image,
                        Corners {
                            top_left: radius,
                            top_right: radius,
                            ..Corners::default()
                        },
                        None,
                    ))
                }),
        )
        .child(
            h_flex()
                .relative()
                .size_full()
                .child(
                    v_flex()
                        .relative()
                        .h_full()
                        .w_12()
                        .p_2()
                        .gap_2()
                        .overflow_hidden()
                        // Kit clamps each radius to half the painted rectangle.
                        // A wider inert fill retains the outer curve in this narrow column.
                        .child(
                            div()
                                .debug_selector({
                                    let id = package.id.clone();
                                    move || format!("theme-preview-sidebar-{id}-{dark}")
                                })
                                .absolute()
                                .left_0()
                                .top_0()
                                .h_full()
                                .w_12()
                                .min_w(radius * 2.)
                                .rounded_tl(radius)
                                .bg(theme.sidebar),
                        )
                        .child(div().size_3().rounded_full().bg(theme.primary))
                        .children(
                            (0..3).map(|_| div().h_1().w_full().bg(theme.foreground.opacity(0.12))),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .p_3()
                        .gap_2()
                        .child(
                            div()
                                .h_2()
                                .w_16()
                                .max_w_full()
                                .rounded_full()
                                .bg(theme.foreground.opacity(0.3)),
                        )
                        .child(div().h_1().w_full().bg(theme.foreground.opacity(0.1)))
                        .child(div().flex_1())
                        .child(
                            h_flex()
                                .h_6()
                                .p_1()
                                .rounded(theme.radius)
                                .bg(theme.group_box)
                                .child(div().flex_1())
                                .child(div().size_4().rounded_full().bg(theme.primary)),
                        ),
                ),
        )
        .into_any_element()
}
