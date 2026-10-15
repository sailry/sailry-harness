//! A compact meter for the conversation button's verified four-bar visual gap.
use super::*;

const BARS: usize = 4;

pub(super) fn render(speech: &Entity<SpeechState>, cx: &App) -> AnyElement {
    let recent: Vec<_> = speech.read(cx).levels().collect();
    let visible = &recent[recent.len().saturating_sub(BARS)..];
    let mut levels = [0.; BARS];
    levels[BARS - visible.len()..].copy_from_slice(visible);
    Waveform { levels }.into_any_element()
}

#[derive(IntoElement)]
struct Waveform {
    levels: [f32; BARS],
}

impl RenderOnce for Waveform {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .id("dictation-waveform")
            .test_support()
            .size_4()
            .items_center()
            .justify_center()
            .gap(px(1.5))
            .children(self.levels.into_iter().enumerate().map(|(index, level)| {
                div()
                    .w(px(2.))
                    .h(px(3. + level.clamp(0., 1.) * 9.))
                    .rounded_full()
                    .bg(cx.theme().foreground)
                    .debug_selector(move || format!("dictation-waveform-bar-{index}"))
            }))
    }
}
