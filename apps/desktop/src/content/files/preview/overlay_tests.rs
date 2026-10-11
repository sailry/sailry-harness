//! Verify tooltip overlay registration without mounting a native WebView.
use core::prelude::v1::test;
use gpui_kit::{
    component::{Root, button::Button, tooltip::Tooltip},
    *,
};
use std::{cell::Cell, rc::Rc, time::Duration};

struct Harness {
    bounds: Bounds<Pixels>,
    hoverable: bool,
    blocked: Rc<Cell<Option<bool>>>,
    clicks: Rc<Cell<usize>>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let bounds = self.bounds;
        let blocked = self.blocked.clone();
        let clicks = self.clicks.clone();
        let builder = move |window: &mut Window, cx: &mut App| {
            let clicks = clicks.clone();
            Tooltip::element(move |_, _| {
                let clicks = clicks.clone();
                div()
                    .debug_selector(|| "preview-tooltip-content".into())
                    .w(px(160.))
                    .h(px(80.))
                    .child(
                        Button::new("tooltip-action")
                            .debug_selector(|| "preview-tooltip-action".into())
                            .label("Choose")
                            .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                    )
            })
            .build(window, cx)
        };
        let trigger = div()
            .id("preview-tooltip-trigger")
            .debug_selector(|| "preview-tooltip-trigger".into())
            .absolute()
            .left(px(48.))
            .top(px(48.))
            .w(px(48.))
            .h(px(32.));
        let trigger = if self.hoverable {
            trigger.hoverable_tooltip(builder)
        } else {
            trigger.tooltip(builder)
        };
        div()
            .relative()
            .size_full()
            .child(
                canvas(
                    move |_, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                    move |_, hitbox, window, _| {
                        blocked.set(Some(window.is_hitbox_occluded(&hitbox)))
                    },
                )
                .size_full(),
            )
            .child(trigger)
    }
}

fn mount(cx: &mut TestAppContext, hoverable: bool) -> (Entity<Harness>, VisualTestContext) {
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness {
            bounds: distant(),
            hoverable,
            blocked: Rc::new(Cell::new(None)),
            clicks: Rc::new(Cell::new(0)),
        });
        owner = Some(view.clone());
        Root::new(view, window, cx)
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(600.), px(400.)));
    frame(visual);
    (owner.unwrap(), visual.clone())
}

fn distant() -> Bounds<Pixels> {
    Bounds::new(point(px(520.), px(320.)), size(px(40.), px(40.)))
}

fn frame(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    visual.run_until_parked();
}

fn show(visual: &mut VisualTestContext) -> Bounds<Pixels> {
    let trigger = visual.debug_bounds("preview-tooltip-trigger").unwrap();
    visual.simulate_mouse_move(trigger.center(), None, Modifiers::default());
    frame(visual);
    visual.executor().advance_clock(Duration::from_millis(501));
    frame(visual);
    tooltip(visual)
}

fn tooltip(visual: &mut VisualTestContext) -> Bounds<Pixels> {
    let bounds = visual
        .debug_bounds("preview-tooltip-content")
        .expect("the real Kit tooltip must be rendered");
    assert!(bounds.size.width > px(0.) && bounds.size.height > px(0.));
    assert!(bounds.left() >= px(0.) && bounds.right() <= px(600.));
    assert!(bounds.top() >= px(0.) && bounds.bottom() <= px(400.));
    bounds
}

fn assert_overlap(view: &Entity<Harness>, visual: &mut VisualTestContext, expected: bool) {
    let tooltip = tooltip(visual);
    let (bounds, blocked) = view.read_with(visual, |view, _| (view.bounds, view.blocked.get()));
    let overlap = bounds.intersect(&tooltip);
    assert_eq!(
        overlap.size.width > px(0.) && overlap.size.height > px(0.),
        expected
    );
    assert_eq!(blocked, Some(expected));
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
}

#[gpui_kit::test]
fn registers_tooltip_overlaps(cx: &mut TestAppContext) {
    init(cx);
    for hoverable in [false, true] {
        let (view, mut visual) = mount(cx, hoverable);
        let bounds = show(&mut visual);
        assert_overlap(&view, &mut visual, false);
        view.update(&mut visual, |view, cx| {
            view.bounds = bounds;
            cx.notify();
        });
        frame(&mut visual);
        assert_overlap(&view, &mut visual, true);
        view.update(&mut visual, |view, cx| {
            view.bounds = distant();
            cx.notify();
        });
        frame(&mut visual);
        assert_overlap(&view, &mut visual, false);
        visual.update(|window, _| window.remove_window());
    }
}

#[gpui_kit::test]
fn hoverable_tooltip_delivers_clicks(cx: &mut TestAppContext) {
    init(cx);
    let (view, mut visual) = mount(cx, true);
    let bounds = show(&mut visual);
    view.update(&mut visual, |view, cx| {
        view.bounds = bounds;
        cx.notify();
    });
    frame(&mut visual);
    assert_overlap(&view, &mut visual, true);
    let action = visual.debug_bounds("preview-tooltip-action").unwrap();
    visual.simulate_mouse_move(action.center(), None, Modifiers::default());
    frame(&mut visual);
    assert_overlap(&view, &mut visual, true);
    visual.simulate_click(action.center(), Modifiers::default());
    frame(&mut visual);
    assert_eq!(view.read_with(&visual, |view, _| view.clicks.get()), 1);
    visual.update(|window, _| window.remove_window());
}
