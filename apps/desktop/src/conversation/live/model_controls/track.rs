//! Kit 0.7's component Slider has no track slot. Compose its Base Slider,
//! Track, Indicator and Thumb to retain pointer handling and value semantics.
//! Only the track decoration is application-owned.
use super::*;
use gpui_kit::base::{Slider, SliderIndicator, SliderThumb, SliderTrack};
use std::time::Duration;

#[cfg(test)]
mod tests;

pub(super) fn render(
    state: &Entity<SliderState>,
    focus: &FocusHandle,
    steps: usize,
    highest: bool,
    disabled: bool,
    cx: &App,
) -> AnyElement {
    let percentage = state.read(cx).value().end() / state.read(cx).max_value();
    let theme = cx.theme();
    let mut surface = theme.popover;
    surface.a = 1.;
    let intensity = if disabled { 0.5 } else { 1. };
    let bar = surface.blend(theme.primary.opacity(intensity));
    let warning = surface.blend(theme.warning.opacity(intensity));
    let danger = surface.blend(theme.danger.opacity(intensity));
    let mut thumb = if theme.is_dark() {
        theme.foreground
    } else {
        theme.slider_thumb
    };
    thumb.a = 1.;
    let thumb = surface.blend(thumb.opacity(intensity));
    let color = if highest { warning } else { bar };
    let track_focus = focus.clone();
    let thumb_focus = focus.clone();
    let fill = div()
        .absolute()
        .left_0()
        .top_0()
        .bottom_0()
        .right(relative(1. - percentage))
        .mr(rems(2. * percentage - 1.))
        .debug_selector(|| "model-effort-fill".into())
        .rounded_l_full()
        .bg(color)
        .when(highest, |fill| {
            fill.bg(linear_gradient(
                90.,
                linear_color_stop(danger, 0.),
                linear_color_stop(warning, 1.),
            ))
        })
        .when(highest && !disabled && !cx.reduce_motion(), |fill| {
            fill.child(
                Flow {
                    phase: 0.,
                    color: gpui_kit::white(),
                }
                .with_animation(
                    "effort-flow",
                    Animation::new(Duration::from_millis(1600))
                        .repeat()
                        .with_max_fps(30.),
                    |mut flow, phase| {
                        flow.phase = phase;
                        flow
                    },
                ),
            )
        });
    Slider::new(state)
        .disabled(disabled)
        .w_full()
        .px_2()
        .child(
            SliderTrack::new(state)
                .disabled(disabled)
                .debug_selector(|| "model-effort-track".into())
                .relative()
                .w_full()
                .h_10()
                .flex()
                .items_center()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    track_focus.focus(window, cx)
                })
                .child(
                    div()
                        .absolute()
                        .debug_selector(|| "model-effort-base".into())
                        .top(px(7.))
                        .w_full()
                        .h(px(26.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(surface.blend(theme.slider_bar.opacity(0.16)))
                        .when(percentage > 0., |track| track.child(fill)),
                )
                .child(
                    // Map values between thumb centers so both ends stay inside the track.
                    SliderIndicator::new(state)
                        .absolute()
                        .left_4()
                        .right_4()
                        .top(px(7.))
                        .h(px(26.))
                        .children((0..steps).map(|index| {
                            div()
                                .absolute()
                                .left(relative(
                                    index as f32 / steps.saturating_sub(1).max(1) as f32,
                                ))
                                .top(relative(0.5))
                                .mt(rems(-0.125))
                                .ml(rems(-0.125))
                                .size_1()
                                .rounded_full()
                                .bg(if theme.is_dark() {
                                    theme.muted_foreground
                                } else {
                                    theme.slider_thumb
                                })
                        }))
                        .child(
                            SliderThumb::new(state)
                                .disabled(disabled)
                                .debug_selector(|| "model-effort-thumb".into())
                                .absolute()
                                .left(relative(percentage))
                                .top(relative(0.5))
                                .mt(rems(-1.))
                                .ml(rems(-1.))
                                .size_8()
                                .rounded_full()
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(thumb)
                                .shadow_sm()
                                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    thumb_focus.focus(window, cx)
                                }),
                        ),
                ),
        )
        .into_any_element()
}

#[derive(IntoElement)]
struct Flow {
    phase: f32,
    color: Hsla,
}

impl RenderOnce for Flow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .debug_selector(|| "model-effort-flow".into())
            .absolute()
            .inset_0()
            .rounded_full()
            .overflow_hidden()
            .children((0..12).map(|index| {
                let phase = (self.phase + index as f32 * 0.0833) % 1.;
                div()
                    .absolute()
                    .left(relative(phase * 1.3 - 0.2))
                    .top(px(3. + ((index * 7) % 16) as f32))
                    .w(px(if index % 3 == 0 {
                        3.
                    } else {
                        12. + (index % 4) as f32 * 5.
                    }))
                    .h(px(if index % 3 == 0 { 3. } else { 1.5 }))
                    .rounded_full()
                    .bg(self.color.opacity(0.55))
            }))
    }
}
