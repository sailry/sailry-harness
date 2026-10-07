use gpui_kit as gpui;
use gpui_kit::{
    AppContext as _, Bounds, Context, Entity, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Pixels, Render, Styled as _, TestAppContext, VisualTestContext, Window,
    div, point, px,
};

use gpui_kit::component::{
    Icon, IconName, Root, Sizable as _, Size, Theme,
    input::{Input, InputState, SelectAll, Textarea, TextareaState},
};

struct Probe {
    input: Entity<InputState>,
    textarea: Entity<TextareaState>,
    size: Size,
    adornments: bool,
    multiline: bool,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let control = if self.multiline {
            Textarea::new(&self.textarea)
                .with_size(self.size)
                .into_any_element()
        } else {
            let mut input = Input::new(&self.input).with_size(self.size);
            if self.adornments {
                input = input
                    .prefix(Icon::new(IconName::Search).size(px(12.)))
                    .suffix(Icon::new(IconName::Check).size(px(12.)));
            }
            input.into_any_element()
        };
        div()
            .w(px(280.))
            .child(control)
            .debug_selector(|| "input-frame".into())
    }
}

fn mount(cx: &mut TestAppContext) -> (Entity<Probe>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let mut probe = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Probe {
            input: cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value("Agj 中文")
                    .placeholder("Agj 中文")
            }),
            textarea: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(2, 4)
                    .default_value("Agj 中文\nSecond line")
            }),
            size: Size::Medium,
            adornments: false,
            multiline: false,
        });
        probe = Some(view.clone());
        Root::new(view, window, cx)
    });
    (probe.unwrap(), cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn frame(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds("input-frame")
        .expect("input frame was not laid out")
}

fn assert_line(probe: &Entity<Probe>, cx: &mut VisualTestContext) {
    let frame = frame(cx);
    let snap_tolerance = cx.update(|window, _| px(0.5 / window.scale_factor()));
    probe.read_with(cx, |probe, cx| {
        let state = probe.input.read(cx);
        let viewport = state.input_bounds();
        let text = state.text_bounds().expect("input text was not laid out");
        let line_height = state.line_height().expect("input line was not laid out");
        let context = format!(
            "size {:?}, adornments {}, value {:?}, frame {frame:?}, viewport {viewport:?}, text {text:?}, line height {line_height:?}",
            probe.size,
            probe.adornments,
            state.value(),
        );
        assert!(viewport.size.height >= line_height, "{context}");
        assert!(text.size.height >= line_height, "{context}");
        assert!(viewport.top() >= frame.top() + px(1.), "{context}");
        assert!(viewport.bottom() <= frame.bottom() - px(1.), "{context}");
        assert!(text.top() >= viewport.top(), "{context}");
        assert!(text.bottom() <= viewport.bottom(), "{context}");
        // GPUI snaps the line origin to the nearest device pixel.
        assert!(
            (viewport.center().y - frame.center().y).abs() <= snap_tolerance,
            "{context}",
        );
        assert!(viewport.size.width > px(0.), "{context}");
    });
}

#[gpui::test]
fn standard_sizes_fit_text_and_placeholders(cx: &mut TestAppContext) {
    let (probe, cx) = mount(cx);
    let input = probe.read_with(cx, |probe, _| probe.input.clone());
    for rem_size in [16., 18.] {
        cx.update(|_, cx| Theme::global_mut(cx).font_size = px(rem_size));
        for (size, height) in [
            (Size::Medium, 2.),
            (Size::Small, 1.5),
            (Size::XSmall, 1.25),
            (Size::Large, 2.75),
        ] {
            for adornments in [false, true] {
                probe.update(cx, |probe, cx| {
                    probe.size = size;
                    probe.adornments = adornments;
                    cx.notify();
                });
                for value in ["Agj 中文", ""] {
                    cx.update(|window, cx| {
                        input.update(cx, |input, cx| input.set_value(value, window, cx));
                    });
                    draw(cx);
                    assert_eq!(
                        frame(cx).size.height,
                        px(rem_size * height),
                        "the size API must retain the control height",
                    );
                    assert_line(&probe, cx);
                }
            }
        }
    }
}

#[gpui::test]
fn centered_bounds_preserve_pointer_and_keyboard_editing(cx: &mut TestAppContext) {
    let (probe, cx) = mount(cx);
    let input = probe.read_with(cx, |probe, _| probe.input.clone());
    for size in [Size::Medium, Size::Small, Size::XSmall, Size::Large] {
        probe.update(cx, |probe, cx| {
            probe.size = size;
            probe.adornments = true;
            cx.notify();
        });
        cx.update(|window, cx| {
            input.update(cx, |input, cx| input.set_value("Agj 中文", window, cx));
        });
        draw(cx);
        let text = input.read_with(cx, |input, _| input.text_bounds().unwrap());
        for y in [
            text.top() + px(0.5),
            text.center().y,
            text.bottom() - px(0.5),
        ] {
            cx.update(|window, cx| window.blur(cx));
            draw(cx);
            cx.simulate_click(point(text.left() + px(0.1), y), Modifiers::default());
            cx.update(|window, cx| {
                assert!(
                    input
                        .read(cx)
                        .presentation()
                        .focus_handle()
                        .is_focused(window),
                    "size {size:?}, line {text:?}, pointer y {y:?}",
                );
            });
            assert_eq!(input.read_with(cx, |input, _| input.cursor()), 0);
        }
        cx.simulate_input("Typed ");
        assert_eq!(
            input.read_with(cx, |input, _| input.value()),
            "Typed Agj 中文"
        );
        cx.update(|window, cx| window.dispatch_action(Box::new(SelectAll), cx));
        cx.simulate_input("Replacement 中文");
        draw(cx);
        assert_eq!(
            input.read_with(cx, |input, _| input.value()),
            "Replacement 中文"
        );
        assert_line(&probe, cx);
        let frame = frame(cx);
        input.read_with(cx, |input, _| {
            let (caret, line_height) = input.cursor_layout().unwrap();
            assert_eq!(caret.size.height, line_height * 0.85);
            assert!(caret.top() >= frame.top() + px(1.));
            assert!(caret.bottom() <= frame.bottom() - px(1.));
        });
    }
}

#[gpui::test]
fn textarea_retains_line_height_and_padding(cx: &mut TestAppContext) {
    let (probe, cx) = mount(cx);
    probe.update(cx, |probe, cx| {
        probe.multiline = true;
        cx.notify();
    });
    for size in [Size::XSmall, Size::Small, Size::Medium, Size::Large] {
        probe.update(cx, |probe, cx| {
            probe.size = size;
            cx.notify();
        });
        draw(cx);
        let frame = frame(cx);
        probe.read_with(cx, |probe, cx| {
            let state = probe.textarea.read(cx);
            let viewport = state.input_bounds();
            assert_eq!(state.line_height(), Some(px(20.)));
            assert_eq!(viewport.size.height, px(40.));
            assert_eq!(viewport.top(), frame.top() + px(1.) + size.input_py());
            assert_eq!(viewport.bottom(), frame.bottom() - px(1.) - size.input_py());
            assert_eq!(viewport.left(), frame.left() + px(1.) + size.input_px());
        });
    }
}
