//! Bezel's bounded Activity pane, on Kit's scrolling and controls.
//! Source: crabtalk/bezel 4a7505a, apps/gallery/src/patterns/agent.rs (MIT).
use crate::{content::markdown, tr};
use gpui_kit::{
    component::{
        button::{Button, ButtonVariants},
        scroll::{ScrollableElement, ScrollableMask},
        *,
    },
    prelude::*,
    *,
};

pub(crate) fn thought(id: String, content: markdown::View) -> impl IntoElement {
    Thought { id, content }
}

#[derive(Default)]
struct Follow {
    scroll: ScrollHandle,
    maximum: Pixels,
    paused: bool,
}

impl Follow {
    fn read_position(&mut self) {
        self.paused = self.maximum > px(0.) && self.scroll.offset().y > -self.maximum + px(1.);
    }

    fn update_extent(&mut self) -> bool {
        let maximum = self.scroll.max_offset().y;
        let changed = maximum != self.maximum;
        self.maximum = maximum;
        changed
    }
}

#[derive(IntoElement)]
struct Thought {
    id: String,
    content: markdown::View,
}

impl RenderOnce for Thought {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state =
            window.use_keyed_state((ElementId::from(self.id.clone()), "follow"), cx, |_, _| {
                Follow::default()
            });
        let (scroll, paused) = state.update(cx, |state, _| {
            state.read_position();
            if !state.paused {
                // Kit resolves this request against the new extent before it
                // prepaints children, including the first rendered frame.
                state.scroll.scroll_to_bottom();
            }
            (state.scroll.clone(), state.paused)
        });
        let measure = state.clone();
        // GPUI uses total ordering for Pixels, where -0px sorts below +0px.
        // A clamped negative zero still means the viewport is at the top.
        let fade = f32::from(scroll.offset().y) < 0.;
        let viewport_id = self.id.clone();
        let content_id = self.id.clone();
        let fade_id = self.id.clone();
        let follow_id = self.id.clone();
        div()
            .min_w_0()
            .w_full()
            .ml(px(10.))
            .pl(px(12.))
            .border_l_1()
            .border_color(cx.theme().border)
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .relative()
                    .w_full()
                    .min_w_0()
                    .child(
                        div()
                            .id(self.id.clone())
                            .debug_selector(move || format!("{viewport_id}-scroll"))
                            .w_full()
                            .min_w_0()
                            .max_h(px(160.))
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .debug_selector(move || format!("{content_id}-content"))
                                    .child(self.content.scroll_handle(scroll.clone())),
                            ),
                    )
                    .vertical_scrollbar(&scroll)
                    // Kit implements this observer as an absolute canvas child.
                    // Keep it outside the tracked content: GPUI includes child
                    // bounds in the scroll extent, even for absolute children.
                    .on_prepaint(move |_, _, cx| {
                        if measure.update(cx, |state, _| state.update_extent()) {
                            // Notify outside drawing so the keyed state's
                            // observer invalidates its owner after clamping.
                            cx.defer(move |cx| measure.update(cx, |_, cx| cx.notify()));
                        }
                    })
                    .child(ScrollableMask::new(Axis::Vertical, &scroll).id(self.id.clone()))
                    .when(fade, |pane| {
                        pane.child(
                            div()
                                .debug_selector(move || format!("{fade_id}-fade"))
                                .absolute()
                                .top_0()
                                .left_0()
                                .w_full()
                                .h(px(20.))
                                .bg(linear_gradient(
                                    180.,
                                    linear_color_stop(cx.theme().background, 0.),
                                    linear_color_stop(cx.theme().background.opacity(0.), 1.),
                                )),
                        )
                    })
                    .when(paused, |pane| {
                        pane.child(
                            div().absolute().bottom_1().right_2().child(
                                Button::new((ElementId::from(self.id), "follow"))
                                    .debug_selector(move || format!("{follow_id}-follow"))
                                    .ghost()
                                    .small()
                                    .icon(IconName::ArrowDown)
                                    .accessibility_label(tr("turn_latest"))
                                    .on_click(move |_, _, cx| {
                                        state.update(cx, |state, cx| {
                                            state.paused = false;
                                            state.scroll.set_offset(point(
                                                state.scroll.offset().x,
                                                -state.maximum,
                                            ));
                                            cx.notify();
                                        })
                                    }),
                            ),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests;
