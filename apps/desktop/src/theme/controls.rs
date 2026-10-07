use super::*;
use core::prelude::v1::test;
use gpui_kit::component::{Root, ThemeMode, button::ButtonVariants as _, v_flex};
use gpui_kit::*;

struct Controls {
    new_session: bool,
    disabled: bool,
    active: bool,
    activations: usize,
}

impl Render for Controls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let button = Button::new("action")
            .primary()
            .label("Action")
            .disabled(self.disabled)
            .debug_selector(|| "theme-send".into())
            .on_click(cx.listener(|view, _, _, cx| {
                view.activations += 1;
                cx.notify();
            }));
        let control = if self.new_session {
            new_session_button(
                button.w_full().icon(gpui_kit::component::IconName::Plus),
                cx,
            )
        } else {
            send_button(button, self.disabled, self.active, cx).into_any_element()
        };
        v_flex().tab_group().child(control)
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
                let mut button = send_button(Button::new("send").primary(), disabled, false, cx);
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
    activation(cx, false);
}

#[gpui::test]
fn new_session_activation(cx: &mut TestAppContext) {
    activation(cx, true);
}

fn activation(cx: &mut TestAppContext, new_session: bool) {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "controls");
    let loaded = package::Loaded::read(&source, true).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        cx.global_mut::<Catalog>().packages.push(loaded.into());
    });
    let mut controls = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Controls {
            new_session,
            disabled: true,
            active: false,
            activations: 0,
        });
        controls = Some(view.clone());
        Root::new(view, window, cx)
    });
    let controls = controls.unwrap();
    for (package, mode) in [
        (0, ThemeMode::Light),
        (0, ThemeMode::Dark),
        (1, ThemeMode::Light),
        (1, ThemeMode::Dark),
    ] {
        visual.update(|window, cx| {
            apply(package, None, cx);
            Theme::change(mode, Some(window), cx);
            controls.update(cx, |view, cx| {
                view.disabled = true;
                view.activations = 0;
                cx.notify();
            });
        });
        settle(visual);
        let bounds = visual.debug_bounds("theme-send").unwrap();
        if new_session && package == 1 {
            let art = visual.debug_bounds("theme-button.new_session").unwrap();
            assert_eq!(art.size, bounds.size);
            assert_eq!(art.top(), bounds.top());
        }
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
fn send_and_stop_assets(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "states");
    let loaded = package::Loaded::read(&source, true).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        cx.global_mut::<Catalog>().packages.push(loaded.into());
        apply(1, None, cx);
    });
    let mut controls = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Controls {
            new_session: false,
            disabled: false,
            active: false,
            activations: 0,
        });
        controls = Some(view.clone());
        Root::new(view, window, cx)
    });
    let controls = controls.unwrap();
    for (active, present, absent) in [
        (false, "theme-icon.send", "theme-icon.stop"),
        (true, "theme-icon.stop", "theme-icon.send"),
        (false, "theme-icon.send", "theme-icon.stop"),
    ] {
        controls.update(visual, |view, cx| {
            view.active = active;
            cx.notify();
        });
        settle(visual);
        assert!(visual.debug_bounds(present).is_some());
        assert!(visual.debug_bounds(absent).is_none());
        let bounds = visual.debug_bounds("theme-send").unwrap();
        visual.simulate_click(bounds.center(), Modifiers::default());
        settle(visual);
    }
    assert_eq!(controls.read_with(visual, |view, _| view.activations), 3);
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

#[gpui::test]
fn composer_action_states(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "actions");
    // Exercise vector fallback as well as the separately tested raster icons.
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("theme.json")).unwrap()).unwrap();
    for mode in ["light", "dark"] {
        for slot in ["icon.send", "icon.stop", "button.send", "button.stop"] {
            manifest[mode]["assets"]
                .as_object_mut()
                .unwrap()
                .remove(slot);
        }
    }
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let loaded = package::Loaded::read(&source, true).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        cx.global_mut::<Catalog>().packages.push(loaded.into());
    });
    let mut controls = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| Controls {
            new_session: false,
            disabled: false,
            active: false,
            activations: 0,
        });
        controls = Some(view.clone());
        cx.new(|cx| Root::new(view, window, cx))
    });
    let controls = controls.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for active in [false, true] {
            let style = visual.update(|window, cx| {
                apply(1, None, cx);
                Theme::change(mode, Some(window), cx);
                controls.update(cx, |view, cx| {
                    view.active = active;
                    cx.notify();
                });
                let composer = &variant(cx).unwrap().composer;
                if active {
                    composer.stop.clone()
                } else {
                    composer.send.clone()
                }
                .unwrap()
            });
            visual.simulate_mouse_move(point(px(300.), px(100.)), None, Modifiers::default());
            settle(visual);
            assert_action_fill(visual, &style.background);
            let bounds = visual.debug_bounds("theme-send").unwrap();
            visual.simulate_mouse_move(bounds.center(), None, Modifiers::default());
            settle(visual);
            assert_action_fill(visual, &style.hover);
            visual.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
            settle(visual);
            assert_action_fill(visual, &style.active);
            visual.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
            settle(visual);
        }
    }
}

fn assert_action_fill(cx: &mut VisualTestContext, color: &str) {
    let expected: Background = gpui_kit::component::theme::try_parse_color(color)
        .unwrap()
        .into();
    cx.update(|window, _| {
        assert!(
            window
                .painted_quads()
                .iter()
                .any(|quad| quad.background == expected),
            "composer action did not paint {color}"
        );
    });
}
