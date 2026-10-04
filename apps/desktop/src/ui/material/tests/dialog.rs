use super::*;
use gpui_kit::component::{
    Root, WindowExt,
    dialog::{DialogAction, DialogFooter},
};
use gpui_kit::{FocusHandle, Modifiers};
use std::time::Duration;

struct Frame {
    alert: bool,
    refined: bool,
    accepted: Rc<Cell<usize>>,
    cancelled: Rc<Cell<usize>>,
}

fn body() -> impl IntoElement {
    div()
        .debug_selector(|| "material-dialog-body".into())
        .w_full()
        .h(px(48.))
        .child("Dialog content")
}

impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let alert = self.alert;
        let refined = self.refined;
        let accepted = self.accepted.clone();
        let cancelled = self.cancelled.clone();
        div().size_full().child(
            Button::new("material-dialog-trigger")
                .label("Open")
                .on_click(move |_, window, cx| {
                    if alert {
                        let accepted = accepted.clone();
                        window.open_alert_dialog(cx, move |dialog, _, _| {
                            let accepted = accepted.clone();
                            dialog
                                .width(px(320.))
                                .when(refined, |dialog| {
                                    dialog.rounded(rems(1.25)).rounded_tr(px(7.))
                                })
                                .title("Confirm")
                                .child(body())
                                .footer(
                                    DialogFooter::new().child(
                                        DialogAction::new().child(
                                            Button::new("material-dialog-confirm")
                                                .debug_selector(|| "material-dialog-confirm".into())
                                                .label("Confirm"),
                                        ),
                                    ),
                                )
                                .on_ok(move |_, _, _| {
                                    accepted.set(accepted.get() + 1);
                                    true
                                })
                        });
                    } else {
                        let cancelled = cancelled.clone();
                        window.open_dialog(cx, move |dialog, _, _| {
                            let cancelled = cancelled.clone();
                            dialog
                                .width(px(320.))
                                .when(refined, |dialog| {
                                    dialog.rounded(rems(1.25)).rounded_tr(px(7.))
                                })
                                .title("Dialog")
                                .child(body())
                                .on_cancel(move |_, _, _| {
                                    cancelled.set(cancelled.get() + 1);
                                    true
                                })
                        });
                    }
                }),
        )
    }
}

fn renderer(cx: &mut TestAppContext) -> Rc<RefCell<Vec<Corners<AbsoluteLength>>>> {
    setup(cx);
    let corners = Rc::new(RefCell::new(Vec::new()));
    cx.update({
        let captured = corners.clone();
        move |cx| {
            gpui_kit::component::surface::set_renderer(
                move |child, corners, _| {
                    captured.borrow_mut().push(corners);
                    div()
                        .debug_selector(|| "material-dialog-surface".into())
                        .child(Surface { child, corners })
                        .into_any_element()
                },
                cx,
            );
        }
    });
    corners
}

fn open(cx: &mut VisualTestContext) -> FocusHandle {
    draw(cx);
    let trigger = cx.update(|window, cx| {
        window.focus_next(cx);
        window.focused(cx).unwrap()
    });
    activate(cx);
    assert!(cx.debug_bounds("dialog-layer").is_some());
    assert!(!cx.update(|window, _| trigger.is_focused(window)));
    trigger
}

fn refined(corners: &RefCell<Vec<Corners<AbsoluteLength>>>) {
    let corners = corners.borrow();
    assert!(
        !corners.is_empty(),
        "the dialog did not use the material renderer"
    );
    for corners in corners.iter() {
        assert_eq!(corners.top_left, rems(1.25).into());
        assert_eq!(corners.top_right, px(7.).into());
        assert_eq!(corners.bottom_left, rems(1.25).into());
        assert_eq!(corners.bottom_right, rems(1.25).into());
    }
}

fn finish_close(cx: &mut VisualTestContext, trigger: &FocusHandle) {
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(300));
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("dialog-layer").is_none());
    assert!(cx.debug_bounds("material-dialog-surface").is_none());
    assert!(cx.update(|window, _| trigger.is_focused(window)));
}

#[gpui::test]
fn default_surface_radius(cx: &mut TestAppContext) {
    for (radius, expected) in [(8., 16.), (12., 24.), (0., 0.)] {
        let corners = renderer(cx);
        cx.update(|cx| Theme::global_mut(cx).radius_lg = px(radius));
        let (_, visual) = cx.add_window_view(|window, cx| {
            let frame = cx.new(|_| Frame {
                alert: false,
                refined: false,
                accepted: Rc::new(Cell::new(0)),
                cancelled: Rc::new(Cell::new(0)),
            });
            Root::new(frame, window, cx)
        });
        let trigger = open(visual);
        assert!(!corners.borrow().is_empty());
        assert!(
            corners.borrow().iter().all(
                |corners| *corners == Corners::all(px(expected)).map(|radius| (*radius).into())
            )
        );
        visual.simulate_keystrokes("escape");
        finish_close(visual, &trigger);
        visual.update(|window, _| window.remove_window());
    }
}

#[gpui::test]
fn motion_and_escape_focus(cx: &mut TestAppContext) {
    let corners = renderer(cx);
    let cancelled = Rc::new(Cell::new(0));
    let (_, visual) = cx.add_window_view({
        let cancelled = cancelled.clone();
        move |window, cx| {
            let frame = cx.new(|_| Frame {
                alert: false,
                refined: true,
                accepted: Rc::new(Cell::new(0)),
                cancelled,
            });
            Root::new(frame, window, cx)
        }
    });
    let trigger = open(visual);
    let opening = visual.debug_bounds("material-dialog-surface").unwrap();
    let opening_body = visual.debug_bounds("material-dialog-body").unwrap();
    assert_eq!(opening.size.width, px(320.));
    assert!(opening.size.height > px(0.));

    // Kit's enter animation uses wall time, not the executor's timer clock.
    std::thread::sleep(Duration::from_millis(300));
    draw(visual);
    let settled = visual.debug_bounds("material-dialog-surface").unwrap();
    let settled_body = visual.debug_bounds("material-dialog-body").unwrap();
    assert_eq!(opening.size, settled.size);
    assert!(opening.top() < settled.top());
    let opening_offset = opening_body.origin - opening.origin;
    let settled_offset = settled_body.origin - settled.origin;
    assert!(f32::from(opening_offset.x - settled_offset.x).abs() < 0.01);
    assert!(f32::from(opening_offset.y - settled_offset.y).abs() < 0.01);
    refined(&corners);

    visual.simulate_keystrokes("escape");
    finish_close(visual, &trigger);
    assert_eq!(cancelled.get(), 1);
}

#[gpui::test]
fn alert_focus(cx: &mut TestAppContext) {
    let corners = renderer(cx);
    let accepted = Rc::new(Cell::new(0));
    let (_, visual) = cx.add_window_view({
        let accepted = accepted.clone();
        move |window, cx| {
            let frame = cx.new(|_| Frame {
                alert: true,
                refined: true,
                accepted,
                cancelled: Rc::new(Cell::new(0)),
            });
            Root::new(frame, window, cx)
        }
    });
    let trigger = open(visual);
    std::thread::sleep(Duration::from_millis(300));
    draw(visual);
    refined(&corners);
    let surface = visual.debug_bounds("material-dialog-surface").unwrap();
    let content = visual.debug_bounds("material-dialog-body").unwrap();
    assert_eq!(surface.size.width, px(320.));
    assert!(surface.contains(&content.center()));

    // AlertDialog keeps its required response semantics under the surface.
    visual.simulate_click(
        point(px(8.), surface.bottom() + px(20.)),
        Modifiers::default(),
    );
    draw(visual);
    assert!(visual.debug_bounds("material-dialog-surface").is_some());
    assert_eq!(accepted.get(), 0);

    let confirm = visual
        .debug_bounds("material-dialog-confirm")
        .unwrap()
        .center();
    visual.simulate_click(confirm, Modifiers::default());
    finish_close(visual, &trigger);
    assert_eq!(accepted.get(), 1);
}
