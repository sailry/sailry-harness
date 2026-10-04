use super::*;
use core::prelude::v1::test;
use gpui_kit as gpui;
use gpui_kit::component::Root;

struct Fixture {
    content: Entity<markdown::State>,
    outer: ScrollHandle,
}

impl Render for Fixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("outer")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.outer)
            .child(div().h(px(24.)))
            .child(thought(
                "activity".into(),
                markdown::View::new(&self.content),
            ))
            .child(div().h(px(1200.)))
    }
}

fn text(lines: usize) -> String {
    (0..lines)
        .map(|line| format!("Line {line} with selectable text\n\n"))
        .collect()
}

fn setup(cx: &mut TestAppContext, lines: usize) -> (Entity<Fixture>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut view = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let fixture = cx.new(|cx| Fixture {
            content: cx.new(|cx| markdown::State::new(text(lines), cx)),
            outer: ScrollHandle::new(),
        });
        view = Some(fixture.clone());
        Root::new(fixture, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(480.), px(400.)));
    (view.unwrap(), visual)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn grow(cx: &mut VisualTestContext, fixture: &Entity<Fixture>, lines: usize) {
    fixture.update(cx, |fixture, cx| {
        fixture
            .content
            .update(cx, |content, cx| content.set_source(text(lines), cx));
        cx.notify();
    });
    draw(cx);
}

fn bounds(cx: &mut VisualTestContext) -> (Bounds<Pixels>, Bounds<Pixels>) {
    (
        cx.debug_bounds("activity-scroll").unwrap(),
        cx.debug_bounds("activity-content").unwrap(),
    )
}

#[track_caller]
fn at_tail(cx: &mut VisualTestContext) {
    let (viewport, content) = bounds(cx);
    assert!(viewport.size.height <= px(160.));
    assert!(
        (content.bottom() - viewport.bottom()).abs() < px(1.),
        "content {content:?} must end at viewport {viewport:?}",
    );
}

fn wheel(cx: &mut VisualTestContext, delta: f32) {
    let position = cx.debug_bounds("activity-scroll").unwrap().center();
    cx.simulate_mouse_move(position, None, Modifiers::default());
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
        ..Default::default()
    });
    draw(cx);
}

#[gpui::test]
fn follows_tail(cx: &mut TestAppContext) {
    let (fixture, visual) = setup(cx, 24);
    // Inspect the first explicitly drawn frame, before a correction redraw.
    visual.update(|window, cx| window.draw(cx).clear(cx));
    at_tail(visual);
    grow(visual, &fixture, 36);
    at_tail(visual);
    draw(visual);
    assert!(visual.debug_bounds("activity-fade").is_some());
    assert!(visual.debug_bounds("activity-follow").is_none());
    assert_eq!(
        fixture.read_with(visual, |fixture, _| fixture.outer.offset().y),
        px(0.)
    );

    grow(visual, &fixture, 1);
    draw(visual);
    let (viewport, content) = bounds(visual);
    assert!(viewport.size.height < px(160.));
    assert_eq!(content.top(), viewport.top());
    assert!(visual.debug_bounds("activity-fade").is_none());
    assert!(visual.debug_bounds("activity-follow").is_none());
}

#[gpui::test]
fn pauses_following(cx: &mut TestAppContext) {
    let (fixture, visual) = setup(cx, 24);
    draw(visual);
    draw(visual);
    let viewport = bounds(visual).0;
    wheel(visual, 64.);
    let paused = bounds(visual).1.top();
    assert!(visual.debug_bounds("activity-follow").is_some());
    assert_eq!(bounds(visual).0, viewport);
    grow(visual, &fixture, 36);
    assert_eq!(bounds(visual).1.top(), paused);

    let button = visual.debug_bounds("activity-follow").unwrap();
    visual.simulate_click(button.center(), Modifiers::default());
    draw(visual);
    at_tail(visual);
    assert!(visual.debug_bounds("activity-follow").is_none());
    grow(visual, &fixture, 42);
    at_tail(visual);

    wheel(visual, 64.);
    assert!(visual.debug_bounds("activity-follow").is_some());
    wheel(visual, -10_000.);
    assert!(visual.debug_bounds("activity-follow").is_none());
    grow(visual, &fixture, 48);
    at_tail(visual);
    assert_eq!(
        fixture.read_with(visual, |fixture, _| fixture.outer.offset().y),
        px(0.)
    );
}

#[gpui::test]
fn preserves_masked_interactions(cx: &mut TestAppContext) {
    let (fixture, visual) = setup(cx, 30);
    draw(visual);
    draw(visual);
    wheel(visual, 80.);
    let viewport = bounds(visual).0;
    assert!(visual.debug_bounds("activity-fade").is_some());
    // Start inside the visual fade: it must not intercept Markdown's pointer.
    let start = point(viewport.left() + px(8.), viewport.top() + px(8.));
    let end = point(viewport.left() + px(150.), viewport.top() + px(60.));
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    draw(visual);
    let selected = fixture.read_with(visual, |fixture, cx| {
        fixture.content.read(cx).selected_text()
    });
    assert!(!selected.is_empty());
    visual.simulate_keystrokes("secondary-c");
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        selected
    );
    assert_eq!(bounds(visual).0, viewport);
    assert_eq!(
        fixture.read_with(visual, |fixture, _| fixture.outer.offset().y),
        px(0.)
    );

    wheel(visual, 10_000.);
    let (viewport, content) = bounds(visual);
    assert_eq!(content.top(), viewport.top());
    assert!(visual.debug_bounds("activity-fade").is_none());
    assert!(visual.debug_bounds("activity-follow").is_some());
    grow(visual, &fixture, 40);
    assert_eq!(bounds(visual).1.top(), viewport.top());
}
