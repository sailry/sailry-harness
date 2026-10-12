use super::*;
use gpui_kit::test::TestWindowExt as _;

struct GroupHarness {
    scroller: Entity<MessageScrollerState>,
    rows: usize,
    diff: bool,
}

impl Render for GroupHarness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows;
        let diff = self.diff;
        div().size_full().child(
            MessageScroller::new("transcript", self.scroller.clone(), move |_, _, cx| {
                v_flex()
                    .w_full()
                    .child(div().h(px(40.)).flex_shrink_0())
                    .child(div().h_6().debug_selector(|| "group-heading".into()))
                    .child(group_scroll(
                        "group-scroll".into(),
                        v_flex()
                            .w_full()
                            .debug_selector(|| "group-content".into())
                            .child(if diff {
                                super::super::diff::file_rows(
                                    "output",
                                    &(0..rows).map(|i| format!("line {i}\n")).collect::<String>(),
                                    "fixture.txt",
                                    true,
                                    Vec::new(),
                                )
                            } else {
                                code(
                                    "output",
                                    &(0..rows).map(|i| format!("line {i}\n")).collect::<String>(),
                                    px(14.),
                                    cx,
                                )
                            })
                            .children((0..rows).map(|index| {
                                div().h_8().flex_shrink_0().child(format!("Tool {index}"))
                            })),
                    ))
                    .child(div().h(px(1200.)).flex_shrink_0())
                    .into_any_element()
            })
            .with_list_style(StyleRefinement::default().py_0()),
        )
    }
}

#[gpui::test]
fn bounds_groups_and_routes_nested_scrolling(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    for (rows, diff) in [(2, false), (30, false), (30, true)] {
        let (view, visual) = cx.add_window_view(|_, cx| GroupHarness {
            scroller: cx.new(|cx| MessageScrollerState::new(1, cx)),
            rows,
            diff,
        });
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(480.), px(600.)));
        draw(visual);
        view.update(visual, |view, cx| {
            view.scroller
                .update(cx, |state, cx| state.scroll_to_item(0, cx));
        });
        draw(visual);
        let viewport = visual.debug_bounds("group-scroll").unwrap();
        let heading = visual.debug_bounds("group-heading").unwrap();
        let position = point(viewport.center().x, viewport.bottom() - px(12.));
        if rows == 2 {
            assert!(viewport.size.height < px(300.));
            wheel(
                visual,
                position,
                ScrollDelta::Pixels(point(px(0.), px(-20.))),
            );
            assert!(visual.debug_bounds("group-scroll").unwrap().top() < viewport.top());
        } else {
            assert_eq!(viewport.size.height, px(300.));
            if diff {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while diff_line(visual).is_none() {
                    draw(visual);
                    assert!(std::time::Instant::now() < deadline, "diff rows deadline");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            let output = output_bounds(visual, diff);
            let text = if diff {
                diff_line(visual).unwrap()
            } else {
                visual.debug_bounds("output-content").unwrap()
            };
            let header = diff.then(|| semantic_bounds(visual, "diff-header-output"));
            wheel(visual, output.center(), ScrollDelta::Lines(point(0., -1.)));
            assert_eq!(visual.debug_bounds("group-scroll").unwrap(), viewport);
            assert_eq!(output_bounds(visual, diff), output);
            assert!(
                if diff {
                    diff_line(visual).unwrap().top()
                } else {
                    visual.debug_bounds("output-content").unwrap().top()
                } < text.top()
            );
            if diff {
                let frame = semantic_bounds(visual, "diff-body-output");
                let card = semantic_bounds(visual, "conversation-diff-output");
                let header = header.unwrap();
                assert_eq!(output, frame);
                assert_eq!(output.left(), card.left() + px(1.));
                assert_eq!(output.right(), card.right() - px(1.));
                assert_eq!(output.top(), header.bottom());
                assert_eq!(semantic_bounds(visual, "diff-header-output"), header);
                assert!(output.bottom() < card.bottom());
                // Exhaust the diff first; only the next wheel reaches the group.
                wheel(
                    visual,
                    output.center(),
                    ScrollDelta::Pixels(point(px(0.), px(-10000.))),
                );
                assert_eq!(output_bounds(visual, diff), output);
                wheel(
                    visual,
                    output.center(),
                    ScrollDelta::Pixels(point(px(0.), px(-20.))),
                );
                assert_eq!(visual.debug_bounds("group-scroll").unwrap(), viewport);
                assert!(output_bounds(visual, diff).top() < output.top());
            }

            let content = visual.debug_bounds("group-content").unwrap();
            wheel(visual, position, ScrollDelta::Lines(point(0., -1.)));
            assert_eq!(visual.debug_bounds("group-scroll").unwrap(), viewport);
            assert_eq!(visual.debug_bounds("group-heading").unwrap(), heading);
            assert!(visual.debug_bounds("group-content").unwrap().top() < content.top());
            wheel(
                visual,
                position,
                ScrollDelta::Pixels(point(px(0.), px(-10000.))),
            );
            assert_eq!(visual.debug_bounds("group-scroll").unwrap(), viewport);
            wheel(
                visual,
                position,
                ScrollDelta::Pixels(point(px(0.), px(-20.))),
            );
            assert!(visual.debug_bounds("group-scroll").unwrap().top() < viewport.top());

            for height in [400., 900.] {
                visual.simulate_window_resize(handle, size(px(480.), px(height)));
                draw(visual);
                view.update(visual, |view, cx| {
                    view.scroller
                        .update(cx, |state, cx| state.scroll_to_item(0, cx));
                });
                draw(visual);
                assert_eq!(
                    visual.debug_bounds("group-scroll").unwrap().size.height,
                    px(GROUP_HEIGHT.min(height * 0.5))
                );
            }
        }
        visual.update(|window, _| window.remove_window());
    }
}

fn semantic_bounds(cx: &mut VisualTestContext, id: &'static str) -> Bounds<Pixels> {
    cx.update(|window, _| window.find(id).bounds())
}
fn output_bounds(cx: &mut VisualTestContext, diff: bool) -> Bounds<Pixels> {
    if diff {
        semantic_bounds(cx, "diff-body-output")
    } else {
        cx.debug_bounds("output-scroll").unwrap()
    }
}
fn diff_line(cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.update(|window, _| {
        window
            .within("diff-body-output")
            .find_all("source")
            .into_iter()
            .find(|line| line.label() == Some("line 1"))
            .map(|line| line.bounds())
    })
}

#[gpui::test]
fn refreshes_diff_hints_after_font_changes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (view, visual) = cx.add_window_view(|_, cx| GroupHarness {
        scroller: cx.new(|cx| MessageScrollerState::new(1, cx)),
        rows: 30,
        diff: true,
    });
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(480.), px(600.)));
    draw(visual);
    view.update(visual, |view, cx| {
        view.scroller
            .update(cx, |state, cx| state.scroll_to_item(0, cx));
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while diff_line(visual).is_none() {
        draw(visual);
        assert!(std::time::Instant::now() < deadline, "diff rows deadline");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let row = diff_line(visual).unwrap();
    let width = output_bounds(visual, true).size.width;
    visual.update(|window, cx| {
        gpui_kit::component::Theme::global_mut(cx).mono_font_size += px(4.);
        window.refresh();
    });
    draw(visual);
    draw(visual);
    assert!(diff_line(visual).unwrap().size.height > row.size.height);
    let output = output_bounds(visual, true);
    assert_eq!(output.size.width, width);
    wheel(
        visual,
        output.center(),
        ScrollDelta::Pixels(point(px(0.), px(-10000.))),
    );
    assert_eq!(output_bounds(visual, true), output);
    wheel(
        visual,
        output.center(),
        ScrollDelta::Pixels(point(px(0.), px(-20.))),
    );
    assert!(output_bounds(visual, true).top() < output.top());
    visual.update(|window, _| window.remove_window());
}
