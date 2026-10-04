use super::*;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::prelude::FluentBuilder as _;

struct ScrollView {
    handle: ScrollHandle,
    axis: Axis,
    sibling: bool,
}

impl Render for ScrollView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let content = div()
            .id("scroll-content")
            .relative()
            .size_full()
            .track_scroll(&self.handle)
            .map(|view| match self.axis {
                Axis::Vertical => view.overflow_y_scroll(),
                Axis::Horizontal => view.overflow_x_scroll(),
            })
            .child(
                div()
                    .w(px(if self.axis == Axis::Horizontal {
                        600.
                    } else {
                        160.
                    }))
                    .h(px(if self.axis == Axis::Vertical {
                        600.
                    } else {
                        160.
                    }))
                    .debug_selector(|| "scroll-content-size".into()),
            )
            .when(!self.sibling, |view| {
                view.scrollbar(&self.handle, self.axis)
            });

        div()
            .relative()
            .w(px(160.))
            .h(px(160.))
            .debug_selector(|| "scroll-viewport".into())
            .child(content)
            .when(self.sibling, |view| view.scrollbar(&self.handle, self.axis))
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn scrollbar_stays_interactive_after_scrolling(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for axis in [Axis::Vertical, Axis::Horizontal] {
        for sibling in [false, true] {
            let handle = ScrollHandle::new();
            let (_, visual) = cx.add_window_view({
                let handle = handle.clone();
                move |_, _| ScrollView {
                    handle,
                    axis,
                    sibling,
                }
            });
            draw(visual);
            let viewport = visual.debug_bounds("scroll-viewport").unwrap();
            assert_eq!(handle.bounds(), viewport);
            let delta = match axis {
                Axis::Vertical => point(px(0.), px(-1000.)),
                Axis::Horizontal => point(px(-1000.), px(0.)),
            };
            visual.simulate_event(ScrollWheelEvent {
                position: viewport.center(),
                delta: ScrollDelta::Pixels(delta),
                ..Default::default()
            });
            draw(visual);
            let before = handle.offset();
            let offset = match axis {
                Axis::Vertical => before.y,
                Axis::Horizontal => before.x,
            };
            assert_eq!(offset, px(-440.));
            assert_eq!(handle.bounds(), viewport);
            let start = viewport.bottom_right() - point(px(10.), px(10.));
            let end = match axis {
                Axis::Vertical => point(start.x, viewport.top() - px(100.)),
                Axis::Horizontal => point(viewport.left() - px(100.), start.y),
            };
            visual.simulate_mouse_move(start, None, Modifiers::default());
            draw(visual);
            visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
            visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
            visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
            draw(visual);
            assert_eq!(
                handle.offset(),
                Point::default(),
                "{axis:?}, sibling={sibling}"
            );
            assert_eq!(handle.bounds(), viewport);
            visual.update(|window, _| window.remove_window());
        }
    }
}
