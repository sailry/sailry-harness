use super::*;
use core::prelude::v1::test;
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};

struct Harness {
    scroller: Entity<MessageScrollerState>,
    lines: usize,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let lines = self.lines;
        div()
            .size_full()
            .child(gpui_kit::base::TextSelectionLayer)
            .child(
                MessageScroller::new("transcript", self.scroller.clone(), move |_, _, cx| {
                    v_flex()
                        .w_full()
                        .child(div().h(px(40.)).flex_shrink_0())
                        .child(code(
                            "output",
                            &(0..lines)
                                .map(|i| format!("line {i}\n"))
                                .collect::<String>(),
                            cx,
                        ))
                        .child(div().h(px(1200.)).flex_shrink_0())
                        .into_any_element()
                })
                .with_list_style(StyleRefinement::default().py_0()),
            )
    }
}

#[gpui::test]
fn isolates_drag_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for lines in [2, 30] {
        let (view, visual) = cx.add_window_view(|_, cx| Harness {
            scroller: cx.new(|cx| MessageScrollerState::new(1, cx)),
            lines,
        });
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(480.), px(600.)));
        draw(visual);
        view.update(visual, |view, cx| {
            view.scroller
                .update(cx, |state, cx| state.scroll_to_item(0, cx));
        });
        draw(visual);
        let viewport = visual.debug_bounds("output-scroll").unwrap();
        let start = point(viewport.left() + px(14.), viewport.top() + px(10.));
        let end = point(viewport.left() + px(100.), viewport.bottom() - px(2.));
        visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
        for _ in 0..80 {
            visual
                .executor()
                .advance_clock(std::time::Duration::from_millis(16));
            draw(visual);
            assert_eq!(visual.debug_bounds("output-scroll").unwrap(), viewport);
        }
        visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        draw(visual);
        let text = visual.update(gpui_kit::base::TextSelection::selected_text);
        assert!(text.contains("line 0"), "selection: {text:?}");
        assert!(
            text.contains(&format!("line {}", lines - 1)),
            "selection: {text:?}"
        );
        wheel(visual, end, ScrollDelta::Pixels(point(px(0.), px(-20.))));
        assert!(visual.debug_bounds("output-scroll").unwrap().top() < viewport.top());
        visual.update(|window, _| window.remove_window());
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn wheel(cx: &mut VisualTestContext, position: Point<Pixels>, delta: ScrollDelta) {
    cx.simulate_mouse_move(position, None, Modifiers::default());
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta,
        ..Default::default()
    });
    draw(cx);
}

#[gpui::test]
fn nested_scroll_limits(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for lines in [2, 30] {
        let (view, visual) = cx.add_window_view(|_, cx| Harness {
            scroller: cx.new(|cx| MessageScrollerState::new(1, cx)),
            lines,
        });
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(480.), px(600.)));
        draw(visual);
        view.update(visual, |view, cx| {
            view.scroller
                .update(cx, |state, cx| state.scroll_to_item(0, cx));
        });
        draw(visual);
        let viewport = visual.debug_bounds("output-scroll").unwrap();
        assert_eq!(viewport.size.height, px(lines.min(5) as f32 * 20.));
        let content = visual.debug_bounds("output-content").unwrap();
        let position = viewport.center();
        wheel(visual, position, ScrollDelta::Lines(point(0., -1.)));
        let after = visual.debug_bounds("output-scroll").unwrap();
        let moved = visual.debug_bounds("output-content").unwrap();
        if lines > 5 {
            assert_eq!(
                after, viewport,
                "inner scrolling must leave the transcript still"
            );
            assert!(moved.top() < content.top(), "the inner output must move");
            wheel(
                visual,
                position,
                ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            );
            assert_eq!(visual.debug_bounds("output-scroll").unwrap(), viewport);
            let bottom = visual.debug_bounds("output-content").unwrap().bottom();
            assert!((bottom - viewport.bottom()).abs() < px(1.));
            wheel(
                visual,
                position,
                ScrollDelta::Pixels(point(px(0.), px(40.))),
            );
            assert_eq!(visual.debug_bounds("output-scroll").unwrap(), viewport);
            assert!(visual.debug_bounds("output-content").unwrap().bottom() > bottom);
            wheel(
                visual,
                position,
                ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            );
            wheel(
                visual,
                position,
                ScrollDelta::Pixels(point(px(0.), px(-20.))),
            );
            assert!(
                visual.debug_bounds("output-scroll").unwrap().top() < viewport.top(),
                "the transcript receives scrolling at the inner boundary"
            );
        } else {
            assert!(
                after.top() < viewport.top(),
                "short output must not trap scrolling"
            );
        }
        let before = visual.debug_bounds("output-scroll").unwrap();
        wheel(
            visual,
            point(px(200.), px(250.)),
            ScrollDelta::Pixels(point(px(0.), px(-10.))),
        );
        assert!(
            visual.debug_bounds("output-scroll").unwrap().top() < before.top(),
            "wheel input outside the output scrolls the transcript"
        );
        visual.update(|window, _| window.remove_window());
    }
}

mod groups;
