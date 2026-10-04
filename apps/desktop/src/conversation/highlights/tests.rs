use super::*;
use core::prelude::v1::test;
use gpui_kit as gpui;
use gpui_kit::component::{Root, Theme, ThemeMode, input::Textarea};
use gpui_kit::*;

#[test]
fn references_and_commands_preserve_source_ranges() {
    let text = "/skill:example:analysis 看 @目录/文件.rs， @project 🙂";
    assert_eq!(
        tokens(text)
            .iter()
            .map(|range| &text[range.clone()])
            .collect::<Vec<_>>(),
        ["/skill:example:analysis", "@目录/文件.rs", "@project"]
    );
    for text in [
        "name@example.com /tmp/file",
        "`@code`",
        "```\n@code\n```",
        "[link](https://example.com/@name)",
        "<span title='@name'>text</span>",
    ] {
        assert!(tokens(text).is_empty(), "unexpected token in {text}");
    }
    assert_eq!(tokens("/").as_slice(), std::slice::from_ref(&(0..1)));
    assert_eq!(tokens("文字 @").as_slice(), std::slice::from_ref(&(7..8)));
}

#[gpui::test]
fn markdown_and_tokens_follow_theme(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        let text = "/skill:example:analysis\n\n# Title\n\n**bold** *italic* `code` [link](file.rs)\n\n看 @目录/文件.rs";
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            Theme::change(mode, None, cx);
            let spans = styles(text, cx);
            let at = |needle: &str| {
                let offset = text.find(needle).unwrap();
                spans.iter().find(|span| span.range.contains(&offset)).unwrap().style
            };
            assert_eq!(at("/skill").color, Some(cx.theme().link));
            assert_eq!(at("@目录").color, Some(cx.theme().link));
            assert_eq!(at("@目录").background_color, None);
            assert_eq!(at("bold").font_weight, Some(FontWeight::BOLD));
            assert_eq!(at("italic").font_style, Some(FontStyle::Italic));
            assert_ne!(at("code"), HighlightStyle::default());
            assert_ne!(at("link"), HighlightStyle::default());
            assert!(spans.iter().all(|span| text.is_char_boundary(span.range.start) && text.is_char_boundary(span.range.end)));
            assert!(spans.windows(2).all(|pair| pair[0].range.end <= pair[1].range.start));
        }
    });
}

#[gpui::test]
fn hides_only_icon_markers(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let text = "看 @SSH @PDF report /skill:reports:analysis";
    let links: Vec<_> = [
        ("@SSH", Some("reicon:ui/puzzle-piece")),
        ("@PDF report", Some("reicon:school/book")),
        ("/skill:reports:analysis", None),
    ]
    .into_iter()
    .map(|(label, icon)| {
        let start = text.find(label).unwrap();
        TextareaLink {
            range: start..start + label.len(),
            id: label.into(),
            icon: icon.map(Into::into),
        }
    })
    .collect();
    let mut input = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let state = cx.new(|cx| TextareaState::new(window, cx));
        state.update(cx, |state, cx| {
            state.set_value(text, window, cx);
            state.set_selected_range(4..4, cx);
        });
        input = Some(state.clone());
        Root::new(state, window, cx)
    });
    let input = input.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, None, cx);
            apply_links(&input, links.clone(), cx);
            window.draw(cx).clear(cx);
            let state = input.read(cx);
            let style_at = |offset| {
                state
                    .decorations()
                    .iter()
                    .find(|span| span.range.contains(&offset))
                    .unwrap()
                    .style
            };
            let base = TextStyle {
                color: cx.theme().foreground,
                ..Default::default()
            };
            for link in &links {
                let marker = style_at(link.range.start);
                let label = style_at(link.range.start + 1);
                assert_eq!(base.clone().highlight(label).color, cx.theme().link);
                if link.icon.is_some() {
                    assert_eq!(marker.fade_out, Some(1.));
                    assert_eq!(base.clone().highlight(marker).color.a, 0.);
                    assert_eq!(label.fade_out, None);
                } else {
                    assert_eq!(marker.fade_out, None);
                    assert_eq!(base.clone().highlight(marker).color, cx.theme().link);
                }
            }
            assert_eq!(state.value(), text);
            assert_eq!(state.selected_range(), 4..4);
            assert_eq!(state.links(), links);
            assert!(state.decorations().iter().all(|span| {
                text.is_char_boundary(span.range.start) && text.is_char_boundary(span.range.end)
            }));
        });
    }
}

struct Composer(Entity<TextareaState>);
impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        apply(&self.0, cx);
        Textarea::new(&self.0)
    }
}

#[gpui::test]
fn preserves_editing_composition_and_undo(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut input = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .submit_on_enter(true)
        });
        input = Some(state.clone());
        let view = cx.new(|cx| {
            cx.observe(&state, |_, _, cx| cx.notify()).detach();
            Composer(state)
        });
        Root::new(view, window, cx)
    });
    let input = input.unwrap();
    visual.update(|window, cx| input.update(cx, |input, cx| input.focus(window, cx)));
    visual.simulate_input("**hello** @文件");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    input.read_with(visual, |input, _| {
        assert_eq!(input.value(), "**hello** @文件");
        assert!(!input.decorations().is_empty());
        assert_eq!(
            input.selected_range(),
            input.value().len()..input.value().len()
        );
    });
    visual.simulate_keystrokes("cmd-a cmd-c");
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        "**hello** @文件"
    );
    visual.update(|_, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string("replacement".into()));
    });
    visual.simulate_keystrokes("cmd-v cmd-z");
    assert_eq!(
        input.read_with(visual, |input, _| input.value()),
        "**hello** @文件"
    );

    visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_value("@", window, cx);
            input.set_selected_range(1..1, cx);
            input.replace_and_mark_text_in_range(None, "中文", Some(2..2), window, cx);
        })
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        input.update(cx, |input, cx| {
            assert_eq!(input.value(), "@中文");
            assert_eq!(input.marked_text_range(window, cx), Some(1..3));
            assert_eq!(input.selected_range(), 7..7);
            assert!(!input.decorations().is_empty());
            input.replace_text_in_range(None, "中文🙂", window, cx);
        });
    });
    visual.run_until_parked();
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        input.update(cx, |input, cx| {
            assert_eq!(input.value(), "@中文🙂");
            assert_eq!(input.marked_text_range(window, cx), None);
            assert_eq!(input.selected_range(), 11..11);
            input.set_value("", window, cx);
        });
    });
    visual.run_until_parked();
    assert!(input.read_with(visual, |input, _| input.decorations().is_empty()));
}

mod wrapping;
