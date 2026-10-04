use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, ThemeMode, button::ButtonVariants as _, v_flex};
use gpui_kit::*;

struct Controls {
    disabled: bool,
    activations: usize,
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().tab_group().child(
            send_button(
                Button::new("send").primary().label("Send"),
                self.disabled,
                cx,
            )
            .debug_selector(|| "theme-send".into())
            .on_click(cx.listener(|view, _, _, cx| {
                view.activations += 1;
                cx.notify();
            })),
        )
    }
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn disabled_fill(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            Theme::change(mode, None, cx);
            for disabled in [false, true] {
                let mut button = send_button(Button::new("send").primary(), disabled, cx);
                let fill = (disabled && !mode.is_dark())
                    .then_some(Fill::from(cx.theme().secondary_active));
                assert_eq!(button.style().background, fill);
                assert_eq!(
                    button.style().border_color,
                    (disabled && !mode.is_dark()).then_some(cx.theme().secondary_active)
                );
            }
        }
    });
}

#[gpui::test]
fn send_activation(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let mut controls = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Controls {
            disabled: true,
            activations: 0,
        });
        controls = Some(view.clone());
        Root::new(view, window, cx)
    });
    let controls = controls.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            controls.update(cx, |view, cx| {
                view.disabled = true;
                view.activations = 0;
                cx.notify();
            });
        });
        settle(visual);
        let bounds = visual.debug_bounds("theme-send").unwrap();
        visual.simulate_click(bounds.center(), Modifiers::default());
        settle(visual);
        assert_eq!(controls.read_with(visual, |view, _| view.activations), 0);
        visual.update(|_, cx| {
            controls.update(cx, |view, cx| {
                view.disabled = false;
                cx.notify();
            });
        });
        settle(visual);
        visual.simulate_click(bounds.center(), Modifiers::default());
        settle(visual);
        assert_eq!(controls.read_with(visual, |view, _| view.activations), 1);
        visual.update(|window, cx| {
            window.blur(cx);
            window.focus_next(cx);
            assert!(window.focused(cx).is_some());
            window.draw(cx).clear(cx);
        });
        for key in ["enter", "space"] {
            let keystroke = Keystroke::parse(key).unwrap();
            visual.simulate_event(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            visual.simulate_event(KeyUpEvent { keystroke });
        }
        settle(visual);
        assert_eq!(controls.read_with(visual, |view, _| view.activations), 3);
    }
}

#[gpui::test]
fn package_switches(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        let builtin = cx.global::<Catalog>().packages[0].clone();
        let mut package = builtin.clone();
        package.id = "custom".into();
        for (config, selected, hover) in [
            (&mut package.light, "#f4e5f218", "#e4f4f518"),
            (&mut package.dark, "#35223530", "#123b4530"),
        ] {
            let colors = &mut Rc::make_mut(config).colors;
            colors.secondary_active = Some(selected.into());
            colors.list_active = Some(selected.into());
            colors.table_active = Some(selected.into());
            colors.accent = Some(hover.into());
            colors.list_hover = Some(hover.into());
            colors.table_hover = Some(hover.into());
        }
        cx.update_global::<Catalog, _>(|catalog, _| catalog.packages.push(package.clone()));
        let before_dark = serde_json::to_value(&builtin.dark.colors).unwrap();
        for mode in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Light] {
            Theme::change(mode, None, cx);
            let defaults = (cx.theme().secondary_active, cx.theme().accent);
            apply(1, None, cx);
            let theme = cx.theme();
            let expected = gpui_kit::component::theme::try_parse_color(if mode.is_dark() {
                "#35223530"
            } else {
                "#f4e5f218"
            })
            .unwrap();
            assert_eq!(theme.secondary_active, expected);
            assert_eq!(theme.list_active, expected);
            assert_eq!(theme.table_active, expected);
            assert_ne!(theme.accent, theme.secondary_active);
            if !mode.is_dark() {
                assert_eq!(sidebar_item(true, false, cx), theme.secondary_active);
                assert_eq!(sidebar_item(false, true, cx), theme.accent);
            }
            apply(0, None, cx);
            assert_eq!((cx.theme().secondary_active, cx.theme().accent), defaults);
            assert_eq!(
                serde_json::to_value(&cx.global::<Catalog>().packages[0].dark.colors).unwrap(),
                before_dark
            );
        }
    });
}
