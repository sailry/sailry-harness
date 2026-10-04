use super::*;

struct Composer(Entity<TextareaState>);

impl Render for Composer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        apply(&self.0, cx);
        div()
            .w_full()
            .child(Textarea::new(&self.0).appearance(false).bordered(false))
    }
}

fn setup(cx: &mut TestAppContext) -> (Entity<TextareaState>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut input = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let state = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 6));
        input = Some(state.clone());
        let view = cx.new(|cx| {
            cx.observe(&state, |_, _, cx| cx.notify()).detach();
            Composer(state)
        });
        Root::new(view, window, cx)
    });
    let input = input.unwrap();
    visual.update(|window, cx| input.update(cx, |input, cx| input.focus(window, cx)));
    (input, visual)
}

fn frame(
    input: &Entity<TextareaState>,
    visual: &mut VisualTestContext,
    edit: impl FnOnce(&mut TextareaState, &mut Window, &mut Context<TextareaState>),
) {
    visual.update(|window, cx| {
        input.update(cx, |input, cx| edit(input, window, cx));
        assert_frame(input, window, cx);
    });
}

fn assert_frame(input: &Entity<TextareaState>, window: &mut Window, cx: &mut App) {
    apply(input, cx);
    // Test updates otherwise flush effects and draw until stable. Inspect the
    // first frame inside the update, before those draws can conceal a jump.
    window.refresh();
    window.draw(cx).clear(cx);
    let input = input.read(cx);
    assert_eq!(
        input.text_bounds().unwrap().left(),
        input.input_bounds().left(),
        "wrapped text shifted horizontally after {:?}",
        input.value()
    );
    assert_eq!(input.scroll_offset().x, px(0.));
}

#[gpui::test]
fn typing_and_deleting(cx: &mut TestAppContext) {
    let (input, visual) = setup(cx);
    for width in [180., 287.5, 480.] {
        visual.simulate_resize(size(px(width), px(800.)));
        for text in [
            "Ordinary text near the soft wrapping boundary. ",
            "**Markdown bold text near the soft wrapping boundary.** ",
            "输入到行末时自动换行，消息内容不应左右移动。",
            "mmmmmmmmmmmmmmmmmmmmmmmmm          ",
        ] {
            frame(&input, visual, |input, window, cx| {
                input.set_value("", window, cx)
            });
            let text = text.repeat(3);
            for ch in text.chars() {
                frame(&input, visual, |input, window, cx| {
                    input.replace_text_in_range(None, &ch.to_string(), window, cx);
                });
            }
            for _ in text.chars() {
                visual.update(|window, cx| {
                    window.dispatch_keystroke(Keystroke::parse("backspace").unwrap(), cx);
                    assert_frame(&input, window, cx);
                });
            }
            assert!(input.read_with(visual, |input, _| input.value().is_empty()));
        }
    }
}

#[gpui::test]
fn composition_and_deferred_scroll(cx: &mut TestAppContext) {
    let (input, visual) = setup(cx);
    visual.simulate_resize(size(px(287.5), px(800.)));
    frame(&input, visual, |input, window, cx| {
        input.set_value("Text near the wrapping edge ", window, cx);
    });
    for text in ["zhong", "中文输入", "中文输入时跨越换行边界"] {
        frame(&input, visual, |input, window, cx| {
            let end = text.encode_utf16().count();
            input.replace_and_mark_text_in_range(None, text, Some(end..end), window, cx);
        });
    }
    frame(&input, visual, |input, window, cx| {
        input.replace_text_in_range(None, "中文输入时跨越换行边界🙂", window, cx);
    });
    let caret = input.read_with(visual, |input, _| input.cursor_layout().unwrap().0.left());
    for x in [-16., 16., -1000.] {
        visual.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.set_scroll_offset(point(px(x), input.scroll_offset().y), cx);
            });
            assert_frame(&input, window, cx);
            assert_eq!(input.read(cx).cursor_layout().unwrap().0.left(), caret);
        });
    }
}

#[gpui::test]
fn unwrapped_text_still_scrolls(cx: &mut TestAppContext) {
    let (input, visual) = setup(cx);
    visual.simulate_resize(size(px(180.), px(800.)));
    visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_soft_wrap(false, window, cx);
            input.replace_text_in_range(None, &"long text ".repeat(20), window, cx);
        });
        window.refresh();
        window.draw(cx).clear(cx);
        let input = input.read(cx);
        let offset = input.scroll_offset().x;
        assert!(offset < px(0.));
        assert_eq!(
            input.text_bounds().unwrap().left(),
            input.input_bounds().left() + offset
        );
        let caret = input.cursor_layout().unwrap().0;
        assert!(caret.left() >= input.input_bounds().left());
        assert!(caret.right() <= input.input_bounds().right());
    });
    frame(&input, visual, |input, window, cx| {
        input.set_soft_wrap(true, window, cx)
    });
}
