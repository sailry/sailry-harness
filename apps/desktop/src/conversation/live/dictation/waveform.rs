//! Kit has no waveform glyph; animate four decorative bars inside its Button.
//! Kit retains focus, activation, sizing, theme and reduced-motion behavior.
use super::*;
use std::{f32::consts::TAU, time::Duration};

pub(super) fn render(cx: &App) -> AnyElement {
    let glyph = Waveform { phase: 0. };
    if cx.reduce_motion() {
        glyph.into_any_element()
    } else {
        glyph
            .with_animation(
                "dictation-waveform",
                Animation::new(Duration::from_millis(800))
                    .repeat()
                    .with_max_fps(30.),
                |mut glyph, phase| {
                    glyph.phase = phase;
                    glyph
                },
            )
            .into_any_element()
    }
}

#[derive(IntoElement)]
struct Waveform {
    phase: f32,
}

impl RenderOnce for Waveform {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .size_5()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .debug_selector(|| "dictation-waveform".into())
            .children((0..4).map(|index| {
                let height = 5. + 11. * ((self.phase + index as f32 * 0.2) * TAU).sin().abs();
                div()
                    .w(px(2.))
                    .h(px(height))
                    .rounded_full()
                    .bg(cx.theme().foreground)
            }))
    }
}
