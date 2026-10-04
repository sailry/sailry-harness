use super::*;
use core::prelude::v1::test;
use gpui_kit::prelude::FluentBuilder as _;

#[test]
fn intersection_visibility() {
    let mask = Bounds::new(point(px(10.), px(10.)), size(px(20.), px(20.)));
    let glyph = |x, y| Bounds::new(point(px(x), px(y)), size(px(16.), px(16.)));
    assert!(visible(glyph(0., 0.), mask));
    assert!(visible(glyph(29., 29.), mask));
    assert!(!visible(glyph(30., 10.), mask));
    assert!(!visible(glyph(-6., 10.), mask));
}

#[derive(Default)]
struct Host {
    hidden: bool,
    waiting: bool,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .overflow_hidden()
            .when(!self.hidden, |view| {
                view.when(!self.waiting, |view| {
                    view.child(crate::ui::loading::grid())
                        .child(crate::ui::loading::mini())
                })
                .child(crate::ui::thinking::render(
                    "thinking-test",
                    if self.waiting {
                        "turn_waiting"
                    } else {
                        "turn_thinking"
                    },
                    px(24.),
                ))
            })
    }
}

fn pending(cx: &TestAppContext) -> usize {
    cx.read(|cx| {
        cx.try_global::<Clock>()
            .map_or(0, |clock| clock.pending.len())
    })
}

fn draw(handle: WindowHandle<Host>, cx: &mut TestAppContext) {
    cx.update_window(handle.into(), |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn setup(cx: &mut TestAppContext) -> WindowHandle<Host> {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let handle = cx.open_window(size(px(100.), px(100.)), |_, _| Host::default());
    handle
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    handle
}

#[gpui_kit::test]
fn shared_timer_lifecycle(cx: &mut TestAppContext) {
    let handle = setup(cx);
    draw(handle, cx);
    assert_eq!(pending(cx), 1);
    handle
        .update(cx, |view, _, cx| {
            view.hidden = true;
            cx.notify();
        })
        .unwrap();
    draw(handle, cx);
    cx.executor().advance_clock(FRAME);
    cx.run_until_parked();
    draw(handle, cx);
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn reduced_motion_stops_scheduling(cx: &mut TestAppContext) {
    let handle = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.executor().advance_clock(FRAME);
    cx.run_until_parked();
    draw(handle, cx);
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn inactive_windows(cx: &mut TestAppContext) {
    let handle = setup(cx);
    draw(handle, cx);
    assert_eq!(pending(cx), 1);
    let other = cx.open_window(size(px(100.), px(100.)), |_, _| Host {
        hidden: true,
        ..Default::default()
    });
    other
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    cx.executor().advance_clock(FRAME);
    cx.run_until_parked();
    draw(handle, cx);
    assert_eq!(pending(cx), 0);
}

#[gpui_kit::test]
fn approval_waiting_animates_without_other_indicators(cx: &mut TestAppContext) {
    let handle = setup(cx);
    handle
        .update(cx, |view, _, cx| {
            view.waiting = true;
            cx.notify();
        })
        .unwrap();
    draw(handle, cx);
    assert_eq!(pending(cx), 1);
    cx.executor().advance_clock(FRAME);
    cx.run_until_parked();
    draw(handle, cx);
    assert_eq!(pending(cx), 1);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.executor().advance_clock(FRAME);
    cx.run_until_parked();
    draw(handle, cx);
    assert_eq!(pending(cx), 0);
}
