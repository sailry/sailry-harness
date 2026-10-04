use core::prelude::v1::test;
use gpui_kit::component::{input::Input, input::InputState, *};
use gpui_kit::{
    component::{
        button::Button,
        form::{Field, Form},
    },
    *,
};

mod scrollbar;

struct Controls(Entity<InputState>);

impl Render for Controls {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .child(
                Button::new("custom")
                    .child(
                        div()
                            .line_height(relative(1.))
                            .debug_selector(|| "label".into())
                            .child("Value"),
                    )
                    .debug_selector(|| "custom-button".into()),
            )
            .child(
                Button::new("button")
                    .label("Value")
                    .debug_selector(|| "button".into()),
            )
            .child(
                Button::new("dropdown")
                    .label("Value")
                    .dropdown_caret(true)
                    .debug_selector(|| "dropdown".into()),
            )
            .child(
                div()
                    .debug_selector(|| "input".into())
                    .child(Input::new(&self.0)),
            )
    }
}

#[gpui::test]
fn normal_controls(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| InputState::new(window, cx).default_value("Value"));
        let controls = cx.new(|_| Controls(input));
        Root::new(controls, window, cx)
    });
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            _ = window.draw(cx);
            assert_eq!(window.rem_size(), px(16.));
        });
        visual.run_until_parked();
        visual.update(|window, cx| {
            _ = window.draw(cx);
        });
        assert_eq!(visual.debug_bounds("label").unwrap().size.height, px(14.));
        for selector in ["button", "custom-button", "dropdown", "input"] {
            assert_eq!(visual.debug_bounds(selector).unwrap().size.height, px(32.));
        }
    }
    visual.update(|window, cx| {
        let theme = Theme::global_mut(cx);
        std::rc::Rc::make_mut(&mut theme.dark_theme).font_size = Some(20.);
        Theme::change(ThemeMode::Dark, Some(window), cx);
        _ = window.draw(cx);
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(visual.debug_bounds("label").unwrap().size.height, px(17.5));
}

#[gpui::test]
fn stacked_field_labels(cx: &mut TestAppContext) {
    struct Fields;
    impl Render for Fields {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            v_flex().child(
                Form::vertical()
                    .child(
                        Field::new()
                            .label("Name")
                            .label_fn(|_, _| {
                                div().debug_selector(|| "first-label".into()).child("Name")
                            })
                            .child(div().debug_selector(|| "first-control".into()).h_8()),
                    )
                    .child(
                        Field::new()
                            .label("Key path")
                            .label_fn(|_, _| {
                                div()
                                    .debug_selector(|| "second-label".into())
                                    .child("Key path")
                            })
                            .child(div().debug_selector(|| "second-control".into()).h_8()),
                    ),
            )
        }
    }
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let fields = cx.new(|_| Fields);
        Root::new(fields, window, cx)
    });
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let first = visual.debug_bounds("first-control").unwrap();
    let second = visual.debug_bounds("second-control").unwrap();
    let first_label = visual.debug_bounds("first-label").unwrap();
    let second_label = visual.debug_bounds("second-label").unwrap();
    assert_eq!(first.left(), first_label.left());
    assert_eq!(first.left(), second.left());
    assert_eq!(first.top() - first_label.bottom(), px(2.));
    assert_eq!(second.top() - second_label.bottom(), px(2.));
    assert_eq!(second_label.top() - first.bottom(), px(10.));
}
