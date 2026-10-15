use super::doc::Container;
use super::*;
use core::prelude::v1::test;
use gpui_kit as gpui;
use gpui_kit::component::Root;
use gpui_kit::{
    AppContext, Context, Entity, Focusable, IntoElement, Render, TestAppContext, VisualTestContext,
    Window, div, prelude::*, px,
};

mod tables;

mod parsing {
    use super::*;

    #[test]
    fn retains_formula_marks_and_partitions() {
        let source =
            "Before $\\frac{x}{2}$ and $y$.\n\n$$\n\\sum_{i=1}^{n}i\n$$\n\n```text\n$x$\n```\n";
        let parsed = with_ranges(source);
        let body = parsed.doc.blocks[0].text_at(Part::Body).unwrap();
        assert_eq!(body.text, "Before \\frac{x}{2} and y.");
        assert_eq!(
            body.marks
                .iter()
                .filter(|mark| mark.mark == Mark::Math)
                .count(),
            2
        );
        assert!(
            matches!(&parsed.doc.blocks[1].kind,BlockKind::Math(text) if text.text=="\\sum_{i=1}^{n}i")
        );
        assert!(matches!(&parsed.doc.blocks[2].kind,BlockKind::Code{code,..} if code.text=="$x$"));
        assert_eq!(
            parsed
                .block_ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<String>(),
            source
        );
    }

    #[test]
    fn keeps_mixed_container_order() {
        let doc = parse(
            "> - first\n>   > quoted\n>   >\n>   > ```rust\n>   > let value = 1;\n>   > ```\n",
        );
        assert_eq!(
            doc.blocks[0].containers,
            vec![Container::Quote(None), Container::List]
        );
        let nested = vec![
            Container::Quote(None),
            Container::List,
            Container::Quote(None),
        ];
        assert_eq!(doc.blocks[1].containers, nested);
        assert_eq!(doc.blocks[2].containers, nested);
        assert!(matches!(doc.blocks[2].kind, BlockKind::Code { .. }));
        let quote_in_list = parse("- > nested\n");
        assert_eq!(quote_in_list.blocks.len(), 1);
        assert_eq!(
            quote_in_list.blocks[0].containers,
            vec![Container::List, Container::Quote(None)]
        );
        assert!(matches!(quote_in_list.blocks[0].kind, BlockKind::Bullet(_)));
    }

    #[test]
    fn separates_numbered_lists() {
        let doc = parse("3. first\n4. next\n\n9) separate\n");
        let numbers: Vec<_> = doc
            .blocks
            .iter()
            .filter_map(|block| match block.kind {
                BlockKind::Ordered { number, .. } => Some(number),
                _ => None,
            })
            .collect();
        assert_eq!(numbers, vec![3, 4, 9]);
    }

    #[test]
    fn preserves_source_ranges() {
        for source in [
            "# Title\r\n\r\n> - **中文**  \r\n>   continuation\r\n\r\n[id]: https://example.com \"Title\"\r\n",
            "- [x] done\n  > | a | b |\n  > | - | - |\n  > | *one* | [two](url) |\n",
            "```unknown\n👩‍💻 é\n```\n\nraw <span>HTML</span>\n",
        ] {
            let parsed = with_ranges(source);
            assert_eq!(parsed.block_ranges.len(), parsed.doc.blocks.len());
            let reconstructed: String = parsed
                .block_ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect();
            assert_eq!(reconstructed, source);
        }
    }
}

mod selection {
    use super::*;

    #[test]
    fn never_splits_extended_graphemes() {
        let source = "A é 👩‍💻 🇨🇳 Z";
        let doc = parse(source);
        let emoji = source.find('👩').unwrap();
        let start = Cursor::new(0, Part::Body, emoji);
        assert_eq!(start.right(&doc).offset, emoji + "👩‍💻".len());
        assert_eq!(start.right(&doc).left(&doc), start);
        assert_eq!(Cursor::new(0, Part::Body, emoji + 5).clamp(&doc), start);
        let combining = source.find('e').unwrap();
        assert_eq!(
            Cursor::new(0, Part::Body, combining + 1).clamp(&doc).offset,
            combining
        );
    }

    #[test]
    fn copies_structured_blocks() {
        let doc = parse("| A | B |\n| - | - |\n| 中文 | 👩‍💻 |\n\n```text\nx\ny\n```\n");
        assert_eq!(
            selectable::copied(&doc, Selection::all(&doc)),
            "A\nB\n中文\n👩‍💻\nx\ny"
        );
        let empty = Doc::default();
        assert_eq!(selectable::copied(&empty, Selection::all(&empty)), "");
    }
}

struct Fixture(Entity<State>);
impl Render for Fixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(400.)).p(px(12.)).child(View::new(&self.0))
    }
}

pub(super) fn setup<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
) -> (Entity<State>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
    });
    let state = cx.new(|cx| State::new(source.to_owned(), cx));
    let state_view = state.clone();
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Fixture(state_view));
        Root::new(view, window, cx)
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    (state, visual)
}

#[gpui::test]
fn keyboard_copy(cx: &mut TestAppContext) {
    let source = "> - **中文** and 👩‍💻\r\n\r\n```rust\r\nlet x = 1;\r\n```\r\n";
    let (state, visual) = setup(cx, source);
    visual.update(|window, cx| {
        state.focus_handle(cx).focus(window, cx);
        assert_eq!(state.read(cx).source(), source);
    });
    visual.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a cmd-c"
    } else {
        "ctrl-a ctrl-c"
    });
    visual.update(|_, cx| {
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            "中文 and 👩‍💻\nlet x = 1;"
        );
        state.update(cx, |state, cx| {
            state.set_source(format!("{source}\nmore"), cx)
        });
        assert_eq!(state.read(cx).source(), format!("{source}\nmore"));
        assert!(!state.read(cx).selected_text().is_empty());
    });
}

#[gpui::test]
fn prepares_once_and_replaces_changed_source(cx: &mut TestAppContext) {
    use super::visuals::{Kind, Renderer, Svg};
    use std::{cell::RefCell, rc::Rc};
    let calls = Rc::new(RefCell::new(Vec::new()));
    let recorded = calls.clone();
    cx.update(|cx|cx.set_global(Renderer(Rc::new(move |request,_,_| {
        recorded.borrow_mut().push((request.kind,request.source));
        gpui_kit::Task::ready(Ok(Svg{source:r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><path d="M0 0L20 20"/></svg>"#.into(),width:20.0,height:20.0,baseline:Some(15.0)}))
    }))));
    let source = "One \\(x^2\\)\n\n\\[\nP(A\\mid B)=\\frac{P(B\\mid A)P(A)}{P(B)}\n\\]\n\n```mermaid\nflowchart LR\nA-->B\n```\n";
    let (state, visual) = setup(cx, source);
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(calls.borrow().len(), 3);
    assert!(calls.borrow().contains(&(Kind::InlineMath, "x^2".into())));
    assert!(calls.borrow().contains(&(
        Kind::DisplayMath,
        r"P(A\mid B)=\frac{P(B\mid A)P(A)}{P(B)}".into()
    )));
    assert!(
        calls
            .borrow()
            .contains(&(Kind::Mermaid, "flowchart LR\nA-->B".into()))
    );
    for _ in 0..3 {
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.run_until_parked();
    }
    assert_eq!(calls.borrow().len(), 3);
    visual.update(|_, cx| state.update(cx, |state, cx| state.set_source("Next $y^3$", cx)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    visual.run_until_parked();
    state.read_with(visual, |state, _| {
        assert_eq!(state.source(), "Next $y^3$");
        assert_eq!(state.visuals.images().len(), 1);
        assert!(
            state
                .visuals
                .images()
                .contains_key(&(Kind::InlineMath, "y^3".into()))
        );
    });
}

#[gpui::test]
fn preserves_diagram_scale_and_direction(cx: &mut TestAppContext) {
    use super::visuals::{Kind, Renderer, Svg};
    use gpui_kit::{point, test::TestWindowExt as _};
    use std::{cell::RefCell, rc::Rc};

    let requests = Rc::new(RefCell::new(Vec::new()));
    let recorded = requests.clone();
    cx.update(|cx| {
        // Prepared geometry exercises the app viewport, not Mermaid's layout engine.
        cx.set_global(Renderer(Rc::new(move |request, _, _| {
            let width = if request.source.starts_with("flowchart LR") {
                1200.
            } else {
                160.
            };
            recorded.borrow_mut().push((request.kind, request.source));
            gpui_kit::Task::ready(Ok(Svg {
                source: format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"120\"><path d=\"M0 0L{width} 120\"/></svg>"
                ),
                width,
                height: 120.,
                baseline: None,
            }))
        })));
    });
    let source = "```mermaid\nflowchart LR\nA-->B\n```\n\n```mermaid\nflowchart TD\nC-->D\n```";
    let (state, visual) = setup(cx, source);
    for _ in 0..2 {
        visual.run_until_parked();
        visual.update(|window, cx| window.render_frame(cx));
    }
    assert_eq!(
        *requests.borrow(),
        [
            (Kind::Mermaid, "flowchart LR\nA-->B".into()),
            (Kind::Mermaid, "flowchart TD\nC-->D".into()),
        ]
    );
    let viewport = visual.update(|window, _| window.find(("diagram-viewport", 0usize)).bounds());
    let wide = visual.debug_bounds("markdown-diagram-image-0").unwrap();
    let narrow = visual.debug_bounds("markdown-diagram-image-1").unwrap();
    let second = visual.update(|window, _| window.find(("diagram-viewport", 1usize)).bounds());
    assert!(viewport.size.width < wide.size.width);
    assert_eq!(wide.size, gpui_kit::size(px(1200.), px(120.)));
    assert_eq!(narrow.size, gpui_kit::size(px(160.), px(120.)));
    assert_eq!(narrow.center().x, second.center().x);
    assert!(narrow.left() >= second.left() && narrow.right() <= second.right());
    assert!(narrow.top() >= second.top() && narrow.bottom() <= second.bottom());
    visual.update(|window, cx| {
        window.scroll(
            ("diagram-viewport", 0usize),
            gpui_kit::ScrollDelta::Pixels(point(px(-600.), px(0.))),
            cx,
        );
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.render_frame(cx));
    let scrolled = visual.debug_bounds("markdown-diagram-image-0").unwrap();
    assert!(scrolled.left() < wide.left() - px(100.));
    assert_eq!(scrolled.size, wide.size);
    assert_eq!(
        visual.debug_bounds("markdown-diagram-image-1").unwrap(),
        narrow
    );
    assert_eq!(
        state.read_with(visual, |state, _| state.source().to_owned()),
        source
    );
    assert_eq!(requests.borrow().len(), 2);
}

struct ImageFixture(Entity<State>);
impl Render for ImageFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(400.))
            .debug_selector(|| "image-markdown".into())
            .child(View::new(&self.0).on_image(|_, _, _, _| {
                div()
                    .w(px(120.))
                    .h(px(80.))
                    .debug_selector(|| "custom-picture".into())
                    .into_any_element()
            }))
    }
}

#[gpui::test]
fn custom_images_keep_only_authored_captions(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
    });
    let state = cx.new(|cx| State::new("![](generated.png)", cx));
    let owner = state.clone();
    let (_, visual) =
        cx.add_window_view(|window, cx| Root::new(cx.new(|_| ImageFixture(owner)), window, cx));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let empty = visual.debug_bounds("image-markdown").unwrap();
    assert_eq!(
        visual.debug_bounds("custom-picture").unwrap().size.height,
        px(80.)
    );
    visual.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.set_source("![Authored caption](generated.png)", cx)
        });
        state.focus_handle(cx).focus(window, cx);
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("image-markdown").unwrap().size.height > empty.size.height);
    visual.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a cmd-c"
    } else {
        "ctrl-a ctrl-c"
    });
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        "Authored caption"
    );
}

#[gpui::test]
fn list_markers_align_with_first_line(cx: &mut TestAppContext) {
    use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
    let source = "1. **中文内容** `code` repeated words that wrap onto another line in the narrow document\n2. Second item\n\n- Bullet item\n\nParagraph\n\nNext paragraph";
    let (_, visual) = setup(cx, source);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| Theme::change(mode, Some(window), cx));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let line_height =
            visual.update(|_, cx| px(typography::Typography::of(cx).body.line_height()));
        for index in 0..3 {
            let marker = visual
                .debug_bounds(Box::leak(
                    format!("markdown-marker-{index}").into_boxed_str(),
                ))
                .unwrap();
            let body = visual
                .debug_bounds(Box::leak(
                    format!("markdown-editor-block-{index}").into_boxed_str(),
                ))
                .unwrap();
            assert_eq!(marker.top(), body.top());
            assert!((marker.size.height - line_height).abs() < px(0.01));
            assert!(marker.right() < body.left());
            if index == 0 {
                assert!(body.size.height > marker.size.height);
            }
        }
        let marker = visual.debug_bounds("markdown-marker-2").unwrap();
        let bullet = visual.debug_bounds("markdown-bullet-2").unwrap();
        assert_eq!(marker.center(), bullet.center());
        let previous = visual.debug_bounds("markdown-editor-block-3").unwrap();
        let next = visual.debug_bounds("markdown-editor-block-4").unwrap();
        let gap = visual.update(|_, cx| cx.theme().spacing_tokens().lg);
        assert_eq!(next.top() - previous.bottom(), gap);
    }
}
