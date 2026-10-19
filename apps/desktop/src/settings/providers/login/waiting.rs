//! One compact authorization status surface for provider browser and device flows.
use super::*;
use gpui_kit::component::group_box::{GroupBox, GroupBoxVariants};

pub(super) fn surface(
    provider: &str,
    status: Option<SharedString>,
    active: bool,
    cx: &App,
) -> impl IntoElement {
    GroupBox::new().w_full().outline().child(
        h_flex()
            .w_full()
            .items_center()
            .gap_3()
            .when(active, |row| row.child(spinner::Spinner::new().small()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_sm().font_medium().child(provider.to_owned()))
                    .when_some(status, |view, status| {
                        view.child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(status),
                        )
                    }),
            ),
    )
}
