use super::*;

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
                                    &super::super::diff::additions(
                                        &(0..rows)
                                            .map(|i| format!("line {i}\n"))
                                            .collect::<String>(),
                                    ),
                                    "fixture.txt",
                                    Vec::new(),
                                    cx,
                                )
                            } else {
                                code(
                                    "output",
                                    &(0..rows).map(|i| format!("line {i}\n")).collect::<String>(),
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
            let output_selector = if diff {
                "diff-viewport"
            } else {
                "output-scroll"
            };
            let content_selector = if diff {
                "diff-line-1"
            } else {
                "output-content"
            };
            let output = visual.debug_bounds(output_selector).unwrap();
            let text = visual.debug_bounds(content_selector).unwrap();
            let header = diff.then(|| visual.debug_bounds("diff-header-output").unwrap());
            wheel(visual, output.center(), ScrollDelta::Lines(point(0., -1.)));
            assert_eq!(visual.debug_bounds("group-scroll").unwrap(), viewport);
            assert_eq!(visual.debug_bounds(output_selector).unwrap(), output);
            assert!(visual.debug_bounds(content_selector).unwrap().top() < text.top());
            if diff {
                let frame = visual.debug_bounds("diff-rows-diff-output").unwrap();
                let card = visual.debug_bounds("conversation-diff-output").unwrap();
                let header = header.unwrap();
                assert_eq!(output, frame);
                assert_eq!(output.left(), card.left() + px(1.));
                assert_eq!(output.right(), card.right() - px(1.));
                assert_eq!(output.top(), header.bottom());
                assert_eq!(visual.debug_bounds("diff-header-output").unwrap(), header);
                assert!(output.bottom() < card.bottom());
                // Exhaust the diff first; only the next wheel reaches the group.
                wheel(
                    visual,
                    output.center(),
                    ScrollDelta::Pixels(point(px(0.), px(-10000.))),
                );
                assert_eq!(visual.debug_bounds(output_selector).unwrap(), output);
                wheel(
                    visual,
                    output.center(),
                    ScrollDelta::Pixels(point(px(0.), px(-20.))),
                );
                assert_eq!(visual.debug_bounds("group-scroll").unwrap(), viewport);
                assert!(visual.debug_bounds(output_selector).unwrap().top() < output.top());
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
