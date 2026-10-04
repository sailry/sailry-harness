//! Session metadata shared by header hover cards and sidebar tooltips.
use gpui_kit::{component::*, *};

pub(crate) fn content(
    id: &str,
    title: SharedString,
    project: SharedString,
    window: &Window,
    cx: &App,
) -> Div {
    let selector = format!("{id}-preview");
    let title_id = format!("{selector}-title");
    let project_id = format!("{selector}-project");
    v_flex()
        .debug_selector(move || selector.clone())
        .w(px(280.).min(window.viewport_size().width - px(48.)))
        .min_w_0()
        .gap_2()
        .child(
            div()
                .debug_selector(move || title_id.clone())
                .w_full()
                .text_sm()
                .font_semibold()
                .whitespace_normal()
                .line_height(rems(1.25))
                .line_clamp(2)
                .max_h(rems(2.5))
                .overflow_hidden()
                .child(title),
        )
        .child(
            div()
                .debug_selector(move || project_id.clone())
                .w_full()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(project),
        )
}
