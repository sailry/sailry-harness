//! Compact four-stop sizing composed from Kit's interactive slider parts.
use crate::tr;
use gpui_kit::{
    base::{Slider, SliderIndicator, SliderThumb, SliderTrack},
    component::{slider::SliderState, tooltip::Tooltip, *},
    *,
};

const LABELS: [&str; 4] = [
    "message_text_small",
    "message_text_standard",
    "message_text_large",
    "message_text_extra_large",
];

pub(super) fn slider(state: &Entity<SliderState>, cx: &App) -> AnyElement {
    let value = state.read(cx).percentage().end;
    let bar = cx.theme().tokens.slider_bar;
    let thumb = cx.theme().tokens.slider_thumb;
    let ring = cx.theme().ring;
    let tooltip_state = state.clone();
    // The standard Slider has fixed thin-track/thumbnail geometry and no mark
    // slots. Its Base parts retain pointer, drag, step and accessibility behavior.
    Slider::new(state)
        .w_full()
        .h_8()
        .child(
            div()
                .id("message-text-size-slider")
                .aria_label(tr("settings_message_text_size"))
                .w_full()
                .h_full()
                .flex()
                .items_center()
                .debug_selector(|| "message-text-size-slider".into())
                .tooltip(move |window, cx| {
                    let size = tooltip_state.read(cx).value().start() as u8;
                    let label = LABELS[usize::from((size - 12) / 2)];
                    Tooltip::element(move |_, _| {
                        div()
                            .debug_selector(|| "message-text-size-tooltip".into())
                            .child(format!(
                                "{} · {}",
                                tr(label),
                                rust_i18n::t!("message_text_size_value", size = size)
                            ))
                    })
                    .build(window, cx)
                })
                .child(
                    SliderTrack::new(state)
                        .relative()
                        .w_full()
                        .h_3()
                        .px_3()
                        .rounded_full_style(cx)
                        .bg(bar.opacity(0.2))
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .h_full()
                                .w(relative(value))
                                .rounded_full_style(cx)
                                .bg(bar),
                        )
                        .child(
                            SliderIndicator::new(state)
                                .relative()
                                .w_full()
                                .h_full()
                                .children(crate::preferences::Data::MESSAGE_TEXT_SIZES.map(
                                    |size| {
                                        div()
                                            .absolute()
                                            .left(relative(f32::from(size - 12) / 6.))
                                            .top_1()
                                            .ml(-px(2.))
                                            .size_1()
                                            .rounded_full_style(cx)
                                            .bg(cx.theme().muted_foreground)
                                            .debug_selector(move || {
                                                format!("message-text-size-tick-{size}")
                                            })
                                    },
                                ))
                                .child(
                                    SliderThumb::new(state)
                                        .absolute()
                                        .left(relative(value))
                                        .top(-px(6.))
                                        .ml(-px(12.))
                                        .size_6()
                                        .rounded_full_style(cx)
                                        .bg(thumb)
                                        .border_1()
                                        .border_color(bar.opacity(0.3))
                                        .shadow_sm()
                                        .hover(move |style| style.border_color(ring))
                                        .debug_selector(|| "message-text-size-thumb".into()),
                                ),
                        ),
                ),
        )
        .into_any_element()
}
