//! Read-only resource details shared by native dialogs and plugin Modal content.
use crate::theme::DialogStyle as _;
use gpui_kit::{
    component::{ActiveTheme as _, WindowExt as _, text::TextView, v_flex},
    *,
};

pub(crate) fn content(
    id: SharedString,
    subtitle: SharedString,
    detail: SharedString,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .id(id.clone())
        .debug_selector(|| "details-content".into())
        .w_full()
        .min_w_0()
        .gap_3()
        .children((!subtitle.is_empty()).then(|| {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(subtitle)
        }))
        .child(TextView::markdown(format!("{id}-text"), detail).selectable(true))
}

pub(crate) fn open(
    title: String,
    subtitle: String,
    detail: String,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        dialog
            .form_title(div().pr_6().child(title.clone()))
            .w((window.viewport_size().width - px(48.))
                .max(px(0.))
                .min(px(560.)))
            .max_h(window.viewport_size().height * 0.8)
            .child(content(
                "details".into(),
                subtitle.clone().into(),
                detail.clone().into(),
                cx,
            ))
    });
}
