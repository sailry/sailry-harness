use super::*;
use core::prelude::v1::test;
use gpui_kit as gpui;
use gpui_kit::component::{
    Root,
    input::{Textarea, TextareaState},
};
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

struct Composer(Entity<TextareaState>);
impl Render for Composer {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
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
}

mod wrapping;
