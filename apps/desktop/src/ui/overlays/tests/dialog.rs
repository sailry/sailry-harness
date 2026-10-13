use super::*;
use gpui_kit::component::{
    Root, WindowExt,
    dialog::{ANIMATION_DURATION, DialogAction, DialogClose, DialogFooter},
};
use gpui_kit::{FocusHandle, Modifiers};

struct Frame {
    alert: bool,
    refined: bool,
    cancel_allowed: Rc<Cell<bool>>,
    accepted: Rc<Cell<usize>>,
    cancelled: Rc<Cell<usize>>,
}

fn body() -> impl IntoElement {
    div()
        .debug_selector(|| "overlay-dialog-body".into())
        .w_full()
        .h(px(48.))
        .child("Dialog content")
}

fn footer() -> impl IntoElement {
    div()
        .debug_selector(|| "overlay-dialog-footer".into())
        .w_full()
        .child(
            DialogFooter::new()
                .child(DialogClose::new().trigger(|button| {
                    button
                        .debug_selector(|| "overlay-dialog-cancel".into())
                        .label("Cancel")
                }))
                .child(
                    DialogAction::new().child(
                        Button::new("overlay-dialog-confirm")
                            .debug_selector(|| "overlay-dialog-confirm".into())
                            .label("Confirm"),
                    ),
                ),
        )
}

impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let alert = self.alert;
        let refined = self.refined;
        let accepted = self.accepted.clone();
        let cancelled = self.cancelled.clone();
        let cancel_allowed = self.cancel_allowed.clone();
        div().size_full().child(
            Button::new("overlay-dialog-trigger")
                .label("Open")
                .on_click(move |_, window, cx| {
                    if alert {
                        let accepted = accepted.clone();
                        let cancelled = cancelled.clone();
                        let cancel_allowed = cancel_allowed.clone();
                        window.open_alert_dialog(cx, move |dialog, _, _| {
                            let accepted = accepted.clone();
                            let cancelled = cancelled.clone();
                            let cancel_allowed = cancel_allowed.clone();
                            dialog
                                .width(px(320.))
                                .when(refined, |dialog| {
                                    dialog.rounded(rems(1.25)).rounded_tr(px(7.))
                                })
                                .title("Confirm")
                                .child(body())
                                .footer(footer())
                                .on_ok(move |_, _, _| {
                                    accepted.set(accepted.get() + 1);
                                    true
                                })
                                .on_cancel(move |_, _, _| {
                                    cancelled.set(cancelled.get() + 1);
                                    cancel_allowed.get()
                                })
                        });
                    } else {
                        let accepted = accepted.clone();
                        let cancelled = cancelled.clone();
                        let cancel_allowed = cancel_allowed.clone();
                        window.open_dialog(cx, move |dialog, _, _| {
                            let accepted = accepted.clone();
                            let cancelled = cancelled.clone();
                            let cancel_allowed = cancel_allowed.clone();
                            dialog
                                .width(px(320.))
                                .when(refined, |dialog| {
                                    dialog.rounded(rems(1.25)).rounded_tr(px(7.))
                                })
                                .title("Dialog")
                                .child(body())
                                .footer(footer())
                                .on_ok(move |_, _, _| {
                                    accepted.set(accepted.get() + 1);
                                    true
                                })
                                .on_cancel(move |_, _, _| {
                                    cancelled.set(cancelled.get() + 1);
                                    cancel_allowed.get()
                                })
                        });
                    }
                }),
        )
    }
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

#[track_caller]
fn finish_close(cx: &mut VisualTestContext, trigger: &FocusHandle) {
    cx.run_until_parked();
    cx.executor().advance_clock(*ANIMATION_DURATION);
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("dialog-layer").is_none());
    assert!(cx.debug_bounds("dialog-0").is_none());
    assert!(cx.update(|window, _| trigger.is_focused(window)));
}

#[gpui::test]
fn footer_close_keeps_intrinsic_width(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    for alert in [false, true] {
        let accepted = Rc::new(Cell::new(0));
        let cancelled = Rc::new(Cell::new(0));
        let cancel_allowed = Rc::new(Cell::new(true));
        let (_, visual) = cx.add_window_view({
            let accepted = accepted.clone();
            let cancelled = cancelled.clone();
            let cancel_allowed = cancel_allowed.clone();
            move |window, cx| {
                let frame = cx.new(|_| Frame {
                    alert,
                    refined: false,
                    cancel_allowed,
                    accepted,
                    cancelled,
                });
                Root::new(frame, window, cx)
            }
        });
        let trigger = open(visual);
        draw(visual);
        let footer = visual.debug_bounds("overlay-dialog-footer").unwrap();
        let cancel = visual.debug_bounds("overlay-dialog-cancel").unwrap();
        let confirm = visual.debug_bounds("overlay-dialog-confirm").unwrap();
        assert_eq!(cancel.center().y, confirm.center().y);
        assert_eq!(cancel.left(), footer.left());
        assert_eq!(confirm.left() - cancel.right(), px(8.));
        let slot = (footer.size.width - px(8.)) / 2.;
        assert!(cancel.size.width < slot);
        assert_eq!(
            confirm.size.width,
            footer.size.width - cancel.size.width - px(8.)
        );
        assert_eq!(confirm.right(), footer.right());
        visual.simulate_click(cancel.center(), Modifiers::default());
        finish_close(visual, &trigger);
        assert_eq!(accepted.get(), 0);

        let trigger = open(visual);
        draw(visual);
        let confirm = visual.debug_bounds("overlay-dialog-confirm").unwrap();
        visual.simulate_click(confirm.center(), Modifiers::default());
        finish_close(visual, &trigger);
        assert_eq!(accepted.get(), 1);

        let trigger = open(visual);
        visual.update(|window, cx| window.press("tab", cx));
        cancel_allowed.set(false);
        activate(visual);
        assert!(visual.debug_bounds("dialog-layer").is_some());
        assert_eq!(accepted.get(), 1);
        assert_eq!(cancelled.get(), 2);
        cancel_allowed.set(true);
        activate(visual);
        finish_close(visual, &trigger);
        assert_eq!(accepted.get(), 1);
        assert_eq!(cancelled.get(), 3);

        let trigger = open(visual);
        visual.update(|window, cx| {
            window.press("tab", cx);
            window.press("space", cx);
        });
        finish_close(visual, &trigger);
        assert_eq!(accepted.get(), 1);
        assert_eq!(cancelled.get(), 4);

        let trigger = open(visual);
        activate(visual);
        finish_close(visual, &trigger);
        assert_eq!(accepted.get(), 2);
        assert_eq!(cancelled.get(), 4);
        visual.update(|window, _| window.remove_window());
    }
}

#[gpui::test]
fn default_surface_radius(cx: &mut TestAppContext) {
    for radius in [8., 12., 0.] {
        setup(cx);
        cx.update(|cx| Theme::global_mut(cx).radius_lg = px(radius));
        let (_, visual) = cx.add_window_view(|window, cx| {
            let frame = cx.new(|_| Frame {
                alert: false,
                refined: false,
                cancel_allowed: Rc::new(Cell::new(true)),
                accepted: Rc::new(Cell::new(0)),
                cancelled: Rc::new(Cell::new(0)),
            });
            Root::new(frame, window, cx)
        });
        let trigger = open(visual);
        let quad = panel(visual, "dialog-0");
        let scale = visual.update(|window, _| window.scale_factor());
        assert_eq!(quad.corner_radii, Corners::all(px(radius)).scale(scale));
        visual.simulate_keystrokes("escape");
        finish_close(visual, &trigger);
        visual.update(|window, _| window.remove_window());
    }
}

#[gpui::test]
fn motion_and_escape_focus(cx: &mut TestAppContext) {
    setup(cx);
    let cancelled = Rc::new(Cell::new(0));
    let (_, visual) = cx.add_window_view({
        let cancelled = cancelled.clone();
        move |window, cx| {
            let frame = cx.new(|_| Frame {
                alert: false,
                refined: true,
                cancel_allowed: Rc::new(Cell::new(true)),
                accepted: Rc::new(Cell::new(0)),
                cancelled,
            });
            Root::new(frame, window, cx)
        }
    });
    let trigger = open(visual);
    let opening = visual.debug_bounds("dialog-0").unwrap();
    let opening_body = visual.debug_bounds("overlay-dialog-body").unwrap();
    assert_eq!(opening.size.width, px(320.));
    assert!(opening.size.height > px(0.));

    // GPUI's supported reduced-motion setting settles the existing animation
    // on its next frame; no wall-clock delay is needed to verify both positions.
    visual.update(|_, cx| cx.set_reduce_motion(true));
    draw(visual);
    let settled = visual.debug_bounds("dialog-0").unwrap();
    let settled_body = visual.debug_bounds("overlay-dialog-body").unwrap();
    assert_eq!(opening.size, settled.size);
    assert!(opening.top() < settled.top());
    let opening_offset = opening_body.origin - opening.origin;
    let settled_offset = settled_body.origin - settled.origin;
    assert!(f32::from(opening_offset.x - settled_offset.x).abs() < 0.01);
    assert!(f32::from(opening_offset.y - settled_offset.y).abs() < 0.01);
    let quad = panel(visual, "dialog-0");
    let scale = visual.update(|window, _| window.scale_factor());
    assert_eq!(quad.corner_radii.top_left, px(20.).scale(scale));
    assert_eq!(quad.corner_radii.top_right, px(7.).scale(scale));
    assert_eq!(quad.corner_radii.bottom_left, px(20.).scale(scale));
    assert_eq!(quad.corner_radii.bottom_right, px(20.).scale(scale));

    visual.simulate_keystrokes("escape");
    finish_close(visual, &trigger);
    assert_eq!(cancelled.get(), 1);
}

#[gpui::test]
fn alert_focus(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let accepted = Rc::new(Cell::new(0));
    let (_, visual) = cx.add_window_view({
        let accepted = accepted.clone();
        move |window, cx| {
            let frame = cx.new(|_| Frame {
                alert: true,
                refined: true,
                cancel_allowed: Rc::new(Cell::new(true)),
                accepted,
                cancelled: Rc::new(Cell::new(0)),
            });
            Root::new(frame, window, cx)
        }
    });
    let trigger = open(visual);
    draw(visual);
    let quad = panel(visual, "dialog-0");
    let scale = visual.update(|window, _| window.scale_factor());
    assert_eq!(quad.corner_radii.top_left, px(20.).scale(scale));
    assert_eq!(quad.corner_radii.top_right, px(7.).scale(scale));
    assert_eq!(quad.corner_radii.bottom_left, px(20.).scale(scale));
    assert_eq!(quad.corner_radii.bottom_right, px(20.).scale(scale));
    let surface = visual.debug_bounds("dialog-0").unwrap();
    let content = visual.debug_bounds("overlay-dialog-body").unwrap();
    assert_eq!(surface.size.width, px(320.));
    assert!(surface.contains(&content.center()));

    // AlertDialog keeps its required response semantics under the surface.
    visual.simulate_click(
        point(px(8.), surface.bottom() + px(20.)),
        Modifiers::default(),
    );
    draw(visual);
    assert!(visual.debug_bounds("dialog-0").is_some());
    assert_eq!(accepted.get(), 0);

    let confirm = visual
        .debug_bounds("overlay-dialog-confirm")
        .unwrap()
        .center();
    visual.simulate_click(confirm, Modifiers::default());
    finish_close(visual, &trigger);
    assert_eq!(accepted.get(), 1);
}
