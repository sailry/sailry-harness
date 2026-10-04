//! Kit has no marquee label. Scroll only the existing row's text with GPUI's
//! measured ScrollHandle; the ListItem still owns all interaction and focus.
use gpui_kit::*;
use std::time::Instant;

#[derive(IntoElement)]
pub(super) struct Title {
    id: SharedString,
    text: SharedString,
    hovered: bool,
}

impl Title {
    pub(super) fn new(id: String, text: SharedString, hovered: bool) -> Self {
        Self {
            id: id.into(),
            text,
            hovered,
        }
    }
}

#[derive(Default)]
struct Motion {
    scroll: ScrollHandle,
    started: Option<Instant>,
    text: SharedString,
}

impl RenderOnce for Title {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state =
            window.use_keyed_state((ElementId::from(self.id.clone()), "motion"), cx, |_, _| {
                Motion::default()
            });
        let animated = self.hovered && !cx.reduce_motion();
        let now = cx.background_executor().now();
        let scroll = state.update(cx, |state, _| {
            if !animated || state.text != self.text {
                state.started = None;
                state.scroll.set_offset(Point::default());
            }
            state.text = self.text.clone();
            if animated {
                let first = state.started.is_none();
                let elapsed = now
                    .duration_since(*state.started.get_or_insert(now))
                    .as_secs_f32();
                let distance = state.scroll.max_offset().x;
                let offset = px((elapsed - 0.6).max(0.) * 32.).min(distance);
                state.scroll.set_offset(point(-offset, px(0.)));
                // One initial frame measures overflow; short labels and completed
                // scrolls never keep a redraw loop alive.
                if first || offset < distance {
                    window.request_animation_frame();
                }
            }
            state.scroll.clone()
        });
        let id = self.id.clone();
        let label = div()
            .id(self.id.clone())
            .debug_selector(move || id.to_string())
            .flex_1()
            .min_w_0();
        if animated {
            let text_id = format!("{}-text", self.id);
            label
                .flex()
                .items_center()
                .overflow_x_scroll()
                .overflow_y_hidden()
                .track_scroll(&scroll)
                .child(
                    div()
                        .debug_selector(move || text_id.clone())
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .child(self.text),
                )
        } else {
            label.truncate().child(self.text)
        }
    }
}

#[cfg(test)]
mod tests;
