use super::*;
use gpui::{AppContext as _, Context, Render, TestAppContext, VisualTestContext, div};

struct Sample {
    text: gpui::Entity<crate::TextViewState>,
}

impl Render for Sample {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(160.)).child(crate::TextSelectionLayer).child(
            crate::TextView::new(&self.text)
                .style(
                    TextViewStyle::default()
                        .with_break_all(true)
                        .with_inline_code_inset(px(4.), px(3.)),
                )
                .selectable(true),
        )
    }
}

#[gpui::test]
fn wrapping_preserves_source_and_graphemes(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let text = "prefix abcdefghijklmnopqrstuvwxyz é👩‍💻".repeat(4);
    let source = text.clone();
    let (view, cx) = cx.add_window_view(|_, cx| Sample {
        text: cx.new(|cx| crate::TextViewState::markdown(&source, cx)),
    });
    let cx: &mut VisualTestContext = cx;
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    view.update(cx, |view, cx| {
        view.text.update(cx, |text, cx| text.select_all(cx))
    });
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let selected = view.read_with(cx, |view, cx| view.text.read(cx).selected_text());
    assert_eq!(selected.trim_end_matches('\n'), text);

    cx.update(|window, _| {
        let items = vec![MeasureItem::Text {
            text: text.clone().into(),
            links: vec![],
            highlights: vec![],
        }];
        let style = window.text_style();
        let ranges = wrapping::break_all(
            &items,
            &[None],
            &[None],
            &style,
            Some(px(160.)),
            false,
            px(0.),
            window,
        );
        assert!(ranges.len() > 4);
        let boundaries = text
            .grapheme_indices(true)
            .map(|(start, _)| start)
            .chain([text.len()])
            .collect::<Vec<_>>();
        for range in &ranges {
            assert!(boundaries.contains(&range.start));
            assert!(boundaries.contains(&range.end));
        }
        assert_eq!(
            ranges
                .iter()
                .map(|range| &text[range.clone()])
                .collect::<String>(),
            text
        );
        // The first line consumes the available width rather than backing up to "prefix ".
        assert!(ranges[0].end > "prefix ".len());
    });
}

#[gpui::test]
fn icon_stays_with_wrapped_label(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|_, cx| Sample {
        text: cx.new(|cx| crate::TextViewState::markdown("sample", cx)),
    });
    cx.update(|window, app| {
        let text: SharedString = "prefix label".into();
        let items = vec![MeasureItem::Text {
            text,
            links: vec![(
                7..12,
                LinkMark {
                    url: "https://example.com".into(),
                    ..Default::default()
                },
            )],
            highlights: vec![],
        }];
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let runs = text_runs("prefix ".len(), &text_style, &[]);
        let prefix = shape_line("prefix ".into(), font_size, &runs, window).width();
        let style = TextViewStyle::default()
            .with_break_all(true)
            .with_link_icon("icons/external-link.svg");
        let layout = layout_styled_flow(
            &items,
            &[None],
            &text_style,
            Some(prefix + font_size * 0.5),
            &style,
            window,
            app,
        );
        let icon = layout
            .fragments
            .iter()
            .find_map(|fragment| match fragment {
                PositionedFragment::Icon { origin, .. } => Some(*origin),
                _ => None,
            })
            .expect("link icon");
        let label = layout
            .fragments
            .iter()
            .find_map(|fragment| match fragment {
                PositionedFragment::Text {
                    source_range,
                    origin,
                    ..
                } if source_range.start == 7 => Some(*origin),
                _ => None,
            })
            .expect("link label");
        assert!(icon.y > px(0.));
        assert!(icon.x < label.x);
        assert_eq!(icon.y + font_size / 2., label.y + window.line_height() / 2.);
    });
}

use unicode_segmentation::UnicodeSegmentation;

#[gpui::test]
fn code_backgrounds_leave_leading_between_wrapped_lines(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|_, cx| Sample {
        text: cx.new(|cx| crate::TextViewState::markdown("sample", cx)),
    });
    cx.update(|window, app| {
        let mut text_style = window.text_style();
        text_style.line_height = relative(1.6);
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        for text in ["updated".repeat(12), "updated\nupdated".into()] {
            let items = vec![MeasureItem::Text {
                highlights: code_highlights(0..text.len()),
                text: text.into(),
                links: vec![],
            }];
            let layout = layout_styled_flow(
                &items,
                &[None],
                &text_style,
                Some(px(80.)),
                &TextViewStyle::default()
                    .with_break_all(true)
                    .with_inline_code_inset(px(6.), px(4.)),
                window,
                app,
            );
            let fragments: Vec<_> = layout
                .fragments
                .iter()
                .filter_map(|fragment| match fragment {
                    PositionedFragment::Text {
                        code_background: Some((background, _)),
                        ..
                    } => Some(*background),
                    _ => None,
                })
                .collect();
            assert!(fragments.len() > 1);
            for pair in fragments.windows(2) {
                let first = pair[0];
                let next = pair[1];
                if next.top() > first.top() {
                    assert!(next.top() - first.bottom() >= font_size * 0.3);
                }
            }
        }
    });
}

#[gpui::test]
fn multi_click_spans_visual_fragments(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let word = "abcdefghijklmnopqrstuvwxyz".repeat(3);
    let source = format!("{word} second word");
    let (view, cx) = cx.add_window_view(|_, cx| Sample {
        text: cx.new(|cx| crate::TextViewState::markdown(&source, cx)),
    });
    let cx: &mut VisualTestContext = cx;
    cx.run_until_parked();
    for (count, expected) in [(2, word.as_str()), (3, source.as_str())] {
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let position = point(px(10.), px(10.));
        cx.simulate_event(gpui::MouseDownEvent {
            position,
            modifiers: Default::default(),
            button: gpui::MouseButton::Left,
            click_count: count,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            position,
            modifiers: Default::default(),
            button: gpui::MouseButton::Left,
            click_count: count,
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        let selected = view.read_with(cx, |view, cx| view.text.read(cx).selected_text());
        assert_eq!(selected.trim_end_matches('\n'), expected);
    }
}

#[gpui::test]
fn drag_copies_contiguous_source(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let source = "abcdefghijklmnopqrstuvwxyz".repeat(3);
    let (view, cx) = cx.add_window_view(|_, cx| Sample {
        text: cx.new(|cx| crate::TextViewState::markdown(&source, cx)),
    });
    let cx: &mut VisualTestContext = cx;
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    cx.simulate_mouse_down(
        point(px(2.), px(8.)),
        gpui::MouseButton::Left,
        Default::default(),
    );
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    cx.simulate_mouse_move(
        point(px(155.), px(300.)),
        Some(gpui::MouseButton::Left),
        Default::default(),
    );
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    cx.simulate_mouse_up(
        point(px(155.), px(300.)),
        gpui::MouseButton::Left,
        Default::default(),
    );
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    let selected = view.read_with(cx, |view, cx| view.text.read(cx).selected_text());
    assert_eq!(selected.trim_end_matches('\n'), source);
}

#[gpui::test]
fn preserves_alignment(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|_, cx| Sample {
        text: cx.new(|cx| crate::TextViewState::markdown("sample", cx)),
    });
    cx.update(|window, app| {
        let items = vec![MeasureItem::Text {
            text: "value".into(),
            links: vec![],
            highlights: vec![],
        }];
        let mut text_style = window.text_style();
        text_style.text_align = gpui::TextAlign::Right;
        let layout = layout_styled_flow(
            &items,
            &[None],
            &text_style,
            Some(px(160.)),
            &TextViewStyle::default().with_break_all(true),
            window,
            app,
        );
        let PositionedFragment::Text { origin, size, .. } = &layout.fragments[0] else {
            panic!("text fragment")
        };
        assert!((origin.x + size.width - px(160.)).abs() < px(0.01));
    });
}

#[gpui::test]
fn code_insets_preserve_source_and_fit_wrapped_lines(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (_, cx) = cx.add_window_view(|_, cx| Sample {
        text: cx.new(|cx| crate::TextViewState::markdown("sample", cx)),
    });
    cx.update(|window, app| {
        let text = "prefix abcdefghijklmnopqrstuvwxyz中文é👩‍💻 suffix";
        let end = text.len() - " suffix".len();
        let items = vec![MeasureItem::Text {
            text: text.into(),
            links: vec![],
            highlights: code_highlights(7..end),
        }];
        for break_all in [false, true] {
            let style = TextViewStyle::default()
                .with_break_all(break_all)
                .with_inline_code_inset(px(4.), px(3.));
            let layout = layout_styled_flow(
                &items,
                &[None],
                &window.text_style(),
                Some(px(100.)),
                &style,
                window,
                app,
            );
            assert!(layout.size.width <= px(100.));
            let source = layout
                .fragments
                .iter()
                .filter_map(|fragment| match fragment {
                    PositionedFragment::Text { text, .. } => Some(text.as_ref()),
                    _ => None,
                })
                .collect::<String>();
            assert_eq!(source, text);
            assert!(
                layout
                    .fragments
                    .iter()
                    .filter(|fragment| matches!(
                        fragment,
                        PositionedFragment::Text {
                            code_background: Some(_),
                            ..
                        }
                    ))
                    .count()
                    > 1
            );
        }
    });
}

fn code_highlights(range: Range<usize>) -> Vec<(Range<usize>, InlineHighlight)> {
    vec![(
        range,
        InlineHighlight {
            style: TextViewStyle::default().inline_code_highlight(),
            font_family: Some("monospace".into()),
            font_size_scale: Some(0.875),
        },
    )]
}

#[gpui::test]
fn padded_code_copies_original_text(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let source = "before **`__missing_file__.txt`** `not_found` after";
    let (view, cx) = cx.add_window_view(|_, cx| Sample {
        text: cx.new(|cx| crate::TextViewState::markdown(source, cx)),
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |view, cx| {
        view.text.update(cx, |text, cx| text.select_all(cx))
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let selected = view.read_with(cx, |view, cx| view.text.read(cx).selected_text());
    assert_eq!(
        selected.trim_end_matches('\n'),
        "before __missing_file__.txt not_found after"
    );
}
