use super::*;
use core::prelude::v1::test;

struct Panel;

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Content {
            id: "status-panel".into(),
            empty: "No items".into(),
            body: None,
            items: (0..48)
                .map(|index| Row {
                    id: format!("status-row-{index}"),
                    text: format!("Item {index}: {}", "Preserve full text 中文 🙂 ".repeat(20)),
                    state: State::Paused,
                    details: true,
                    highlight: false,
                })
                .collect(),
            footer: vec![
                button::Button::new("status-action")
                    .label("Clear")
                    .into_any_element(),
            ],
        }
    }
}

#[gpui::test]
fn keeps_footer_outside_bounded_rows(cx: &mut TestAppContext) {
    crate::conversation::live::tests::fixture::init(cx);
    let (_, visual) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|_| Panel);
        Root::new(panel, window, cx)
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let panel = visual.debug_bounds("status-panel").unwrap();
    let scroll = visual.debug_bounds("status-panel-scroll").unwrap();
    let footer = visual.debug_bounds("status-panel-footer").unwrap();
    let first = visual.debug_bounds("status-row-0").unwrap();
    let label = visual.debug_bounds("status-row-0-label").unwrap();
    let caret = visual.debug_bounds("status-row-0-caret").unwrap();
    assert!(caret.left() >= label.right());
    assert!(first.contains(&caret.center()));
    let last = visual.debug_bounds("status-row-47").unwrap();
    assert_eq!(first.size.height, px(32.));
    assert_eq!(last.size.height, px(32.));
    assert!(last.bottom() > scroll.bottom());
    assert!(scroll.size.height <= px(320.));
    assert!(footer.top() >= scroll.bottom());
    assert!(footer.bottom() <= panel.bottom());
    visual.update(|window, cx| {
        window.focus_next(cx);
        assert!(window.focused(cx).is_some());
        window.draw(cx).clear(cx);
    });
    press(visual, "enter");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("status-row-0-details").is_some());
    assert!(
        visual
            .debug_bounds("status-details-status-row-0-content")
            .is_some()
    );
    let expanded_footer = visual.debug_bounds("status-panel-footer").unwrap();
    assert_eq!(expanded_footer, footer);
    press(visual, "enter");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("status-row-0-details").is_none());
    visual.update(|window, _| window.remove_window());
}

fn press(visual: &mut VisualTestContext, key: &str) {
    let keystroke = Keystroke::parse(key).unwrap();
    visual.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    visual.simulate_event(KeyUpEvent { keystroke });
}

struct Activity {
    state: State,
}

impl Render for Activity {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Rows {
            id: "activity-list".into(),
            items: vec![Row {
                id: "activity-row".into(),
                text: "Read source".into(),
                state: self.state,
                details: false,
                highlight: false,
            }],
        }
    }
}

#[gpui::test]
fn animates_only_active_rows(cx: &mut TestAppContext) {
    crate::conversation::live::tests::fixture::init(cx);
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Activity {
            state: State::Pending,
        });
        entity = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = entity.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for reduced in [false, true] {
            visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                cx.set_reduce_motion(reduced);
            });
            for state in [
                State::Pending,
                State::InProgress,
                State::Completed,
                State::Failed,
            ] {
                view.update(visual, |view, cx| {
                    view.state = state;
                    cx.notify();
                });
                let frames = visual.update(|window, cx| {
                    window.simulate_next_frame(cx);
                    window.refresh();
                    window.draw(cx).clear(cx);
                    window.simulate_next_frame(cx)
                });
                assert_eq!(frames > 0, state == State::InProgress && !reduced);
                assert_eq!(
                    visual.debug_bounds("activity-row").unwrap().size.height,
                    px(32.)
                );
            }
        }
    }
    visual.update(|window, _| window.remove_window());
}

struct Tasks;

impl Render for Tasks {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(320.)).child(Content {
            id: "task-panel".into(),
            empty: "No tasks".into(),
            body: None,
            items: (0..4)
                .map(|index| Row {
                    id: format!("task-row-{index}"),
                    text: "Preserve full task 中文 🙂 e\u{301} ".repeat(40),
                    state: if index == 0 {
                        State::InProgress
                    } else {
                        State::Pending
                    },
                    details: false,
                    highlight: index == 0,
                })
                .collect(),
            footer: Vec::new(),
        })
    }
}

#[gpui::test]
fn constrains_plain_and_highlighted_labels(cx: &mut TestAppContext) {
    crate::conversation::live::tests::fixture::init(cx);
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Tasks);
        Root::new(view, window, cx)
    });
    visual.update(|_, cx| cx.set_reduce_motion(true));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let panel = visual.debug_bounds("task-panel").unwrap();
    for index in 0..4 {
        let row_key = Box::leak(format!("task-row-{index}").into_boxed_str());
        let label_key = Box::leak(format!("task-row-{index}-label").into_boxed_str());
        let icon_key = Box::leak(format!("task-row-{index}-icon").into_boxed_str());
        let row = visual.debug_bounds(row_key).unwrap();
        let label = visual.debug_bounds(label_key).unwrap();
        let icon = visual.debug_bounds(icon_key).unwrap();
        assert_eq!(row.size.height, px(32.));
        assert!(label.right() <= row.right() - px(8.));
        assert!(row.right() <= panel.right());
        assert!(label.left() >= icon.right() + px(8.));
    }
    visual.update(|window, _| window.remove_window());
}

struct Prose {
    text: SharedString,
    rows: bool,
}

impl Render for Prose {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Content {
            id: "prose-panel".into(),
            empty: "No items".into(),
            body: Some(self.text.clone()),
            items: if self.rows {
                vec![Row {
                    id: "prose-row".into(),
                    text: "A separate item".into(),
                    state: State::Pending,
                    details: false,
                    highlight: false,
                }]
            } else {
                Vec::new()
            },
            footer: vec![
                button::Button::new("prose-action")
                    .label("Clear")
                    .into_any_element(),
            ],
        }
    }
}

#[gpui::test]
fn plain_body_shares_scroll_not_footer(cx: &mut TestAppContext) {
    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};

    crate::conversation::live::tests::fixture::init(cx);
    let text: SharedString = "## Literal **Markdown** 中文 🙂\n\nPreserve full prose e\u{301}\n"
        .repeat(24)
        .into();
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Prose {
            text: text.clone(),
            rows: false,
        });
        entity = Some(view.clone());
        Root::new(view, window, cx)
    });
    let view = entity.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for rows in [false, true] {
            visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                view.update(cx, |view, cx| {
                    view.rows = rows;
                    cx.notify();
                });
                window.draw(cx).clear(cx);
            });
            visual.run_until_parked();
            visual.update(|window, cx| {
                window.render_frame(cx);
                let label = window.find_by_label(text.as_str());
                assert_eq!(label.role(), Some(Role::Label));
            });
            let scroll = visual.debug_bounds("prose-panel-scroll").unwrap();
            let body = visual.debug_bounds("prose-panel-body").unwrap();
            let footer = visual.debug_bounds("prose-panel-footer").unwrap();
            assert!(body.size.height > scroll.size.height);
            assert!(scroll.size.height <= px(320.));
            assert!(body.right() <= scroll.right());
            assert!(footer.top() >= scroll.bottom());
            assert_eq!(visual.debug_bounds("prose-row").is_some(), rows);
            assert!(visual.debug_bounds("prose-row-details").is_none());
        }
    }
    visual.update(|window, _| window.remove_window());
}
