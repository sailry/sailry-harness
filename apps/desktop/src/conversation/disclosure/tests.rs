use super::*;
use core::prelude::v1::test;
use gpui_kit::component::collapsible::Collapsible;

#[test]
fn preserves_full_wrapped_accessible_labels() {
    let label: SharedString =
        "Complete prompt with Unicode 中文 🙂\nPreserve the final line".into();
    let detail = Detail {
        wrap: true,
        state: Some("Awaiting answer".into()),
        ..Default::default()
    };
    assert_eq!(
        detail.accessible(&label),
        "Complete prompt with Unicode 中文 🙂\nPreserve the final line Awaiting answer"
    );
}

#[gpui::test]
fn colors_failure_text_semantically(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            Theme::change(mode, None, cx);
            let detail = Detail::mono("missing.txt").state(Some("Read failed".into()), true);
            assert_eq!(tone(&detail, cx), cx.theme().danger);
            assert_ne!(tone(&Detail::default(), cx), cx.theme().danger);
            assert_ne!(
                tone(
                    &Detail {
                        pending: true,
                        ..Default::default()
                    },
                    cx
                ),
                cx.theme().danger
            );
            assert_eq!(
                detail.accessible(&"Read".into()),
                "Read missing.txt Read failed"
            );
            assert_eq!(
                tone(
                    &Detail {
                        running: true,
                        ..detail
                    },
                    cx
                ),
                cx.theme().danger
            );
        }
    });
}

struct Harness {
    open: bool,
    detail: SharedString,
    failed: bool,
    running: bool,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(560.)).child(
            Collapsible::new()
                .open(self.open)
                .w_full()
                .child(
                    trigger(
                        "activity".into(),
                        IconName::BookOpen,
                        "Read".into(),
                        Detail {
                            text: self.detail.clone(),
                            failed: self.failed,
                            running: self.running,
                            ..Default::default()
                        },
                        self.open,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open = !this.open;
                        cx.notify();
                    })),
                )
                .content(div().debug_selector(|| "output".into()).w_full().h_20()),
        )
    }
}

#[gpui::test]
fn preserves_inline_alignment(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness {
            open: false,
            detail: "source.txt".into(),
            failed: false,
            running: false,
        });
        Root::new(view, window, cx)
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            window.draw(cx).clear(cx);
        });
        let collapsed = visual.debug_bounds("activity-summary").unwrap();
        let trigger = visual.debug_bounds("activity").unwrap();
        assert_eq!(collapsed.left(), trigger.left());
        assert!(trigger.size.width < px(300.));
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let expanded = visual.debug_bounds("activity-summary").unwrap();
        let output = visual.debug_bounds("output").unwrap();
        assert_eq!(expanded.left(), collapsed.left());
        assert_eq!(expanded.left(), output.left());
        assert_eq!(expanded.size, collapsed.size);
        visual.simulate_click(trigger.center(), Modifiers::default());
        visual.run_until_parked();
    }
}

#[gpui::test]
fn long_details_fit_the_column(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness {
            open: false,
            detail: "A long command with arguments ".repeat(100).into(),
            failed: false,
            running: false,
        });
        Root::new(view, window, cx)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let trigger = visual.debug_bounds("activity").unwrap();
    let summary = visual.debug_bounds("activity-summary").unwrap();
    assert!(
        trigger.right() <= px(560.),
        "trigger exceeds column: {trigger:?}"
    );
    assert!(
        summary.right() <= px(560.),
        "summary exceeds column: {summary:?}"
    );
    assert_eq!(trigger.size.height, px(24.));
}

#[gpui::test]
fn failed_continuing_summary_stays_static(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Harness {
            open: false,
            detail: "missing.txt".into(),
            failed: true,
            running: true,
        });
        Root::new(view, window, cx)
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            window.draw(cx).clear(cx);
        });
        assert!(visual.debug_bounds("activity-summary-shimmer").is_none());
        let bounds = visual.debug_bounds("activity-summary").unwrap();
        visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(visual.debug_bounds("activity-summary").unwrap(), bounds);
        visual.simulate_click(bounds.center(), Modifiers::default());
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("output").is_some());
        visual.simulate_click(bounds.center(), Modifiers::default());
        visual.run_until_parked();
    }
    visual.update(|window, _| window.remove_window());
}

struct AgentIcon {
    rotating: bool,
    pending: bool,
}

impl Render for AgentIcon {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        trigger(
            "child-activity".into(),
            crate::ui::identicon::agent("child", cx),
            "Child".into(),
            Detail {
                // Keep text still so frame requests prove the icon itself rotates.
                rotate_icon: self.rotating,
                pending: self.pending,
                ..Default::default()
            },
            false,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.rotating = !this.rotating;
            cx.notify();
        }))
    }
}

#[gpui::test]
fn rotates_only_when_requested(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| AgentIcon {
            rotating: false,
            pending: false,
        });
        entity = Some(view.clone());
        Root::new(view, window, cx)
    });
    let entity = entity.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for reduced in [false, true] {
            visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                cx.set_reduce_motion(reduced);
            });
            let mut resting = None;
            for (rotating, pending) in [(false, false), (true, false), (true, true), (false, false)]
            {
                entity.update(visual, |view, cx| {
                    view.rotating = rotating;
                    view.pending = pending;
                    cx.notify();
                });
                let frames = visual.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.refresh();
                    window.draw(cx).clear(cx);
                    window.simulate_next_frame(cx)
                });
                assert_eq!(frames > 0, rotating && !pending && !reduced);
                let bounds = visual.debug_bounds("child-activity").unwrap();
                assert_eq!(bounds.size.height, px(24.));
                if !pending {
                    if let Some(resting) = resting {
                        assert_eq!(bounds, resting);
                    } else {
                        resting = Some(bounds);
                    }
                }
            }
        }
    }
    let position = visual.debug_bounds("child-activity").unwrap().center();
    visual.simulate_click(position, Modifiers::default());
    visual.run_until_parked();
    assert!(entity.read_with(visual, |view, _| view.rotating));
    visual.update(|window, _| window.remove_window());
}
