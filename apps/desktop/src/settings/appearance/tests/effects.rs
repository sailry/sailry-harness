use super::*;

struct Screen {
    workspace: Entity<Workspace>,
    clicks: usize,
}

impl Render for Screen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .relative()
            .child(self.workspace.clone())
            .child(
                Button::new("effect-action")
                    .label("Action")
                    .absolute()
                    .bottom_0()
                    .right_0()
                    .debug_selector(|| "effect-action".into())
                    .on_click(cx.listener(|screen, _, _, cx| {
                        screen.clicks += 1;
                        cx.notify();
                    })),
            )
            // Deliberately cover the action to verify decoration never captures it.
            .children(theme::effects::layer("background.window", cx))
    }
}

#[gpui::test]
fn toggles_and_preserves_input(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let source = theme::fixture::write(&directory.path().join("desktop/themes"), "tide");
    let mut value = theme::fixture::manifest("tide");
    for mode in ["light", "dark"] {
        value[mode]["effects"] = serde_json::json!({"background.window": [{
            "kind": "twinkle", "color": "#80DFFF", "count": 12,
            "opacity": 0.2, "size": 2.0, "duration": 12.0
        }]});
    }
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let path = directory.path().join("desktop/preferences.json");
    cx.update(|cx| {
        cx.set_global(crate::preferences::Preferences::open(path.clone()));
        gpui_kit::init(cx);
        theme::init(cx);
    });
    let mut screen = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        theme::apply(1, Some(window), cx);
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |view, cx| {
            view.select(super::super::super::Section::Appearance, cx)
        });
        let view = cx.new(|_| Screen {
            workspace,
            clicks: 0,
        });
        screen = Some(view.clone());
        Root::new(view, window, cx)
    });
    let screen = screen.unwrap();
    settle(visual);
    assert!(
        visual
            .debug_bounds("theme-effects-background.window")
            .is_some()
    );
    click(visual, "effect-action");
    visual.update(|_, cx| assert_eq!(screen.read(cx).clicks, 1));
    click(visual, "appearance-effects");
    assert!(
        visual
            .debug_bounds("theme-effects-background.window")
            .is_none()
    );
    assert_eq!(
        crate::preferences::Preferences::open(path.clone())
            .data
            .theme_effects,
        Some(false)
    );
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| theme::select(Some(mode), window, cx));
        settle(visual);
        assert!(
            visual
                .debug_bounds("theme-effects-background.window")
                .is_none()
        );
        visual.update(|_, cx| assert_eq!(cx.global::<theme::Catalog>().selected, 1));
    }
    for index in [0, 1] {
        visual.update(|window, cx| theme::apply(index, Some(window), cx));
        settle(visual);
        assert!(
            visual
                .debug_bounds("theme-effects-background.window")
                .is_none()
        );
        visual.update(|_, cx| assert_eq!(crate::preferences::data(cx).theme_effects, Some(false)));
    }
    click(visual, "appearance-effects");
    assert!(
        visual
            .debug_bounds("theme-effects-background.window")
            .is_some()
    );
    visual.update(|_, cx| cx.set_reduce_motion(true));
    settle(visual);
    assert!(
        visual
            .debug_bounds("theme-effects-background.window")
            .is_none()
    );
    assert_eq!(
        crate::preferences::Preferences::open(path)
            .data
            .theme_effects,
        Some(true)
    );
    visual.update(|_, cx| cx.set_reduce_motion(false));
    settle(visual);
    assert!(
        visual
            .debug_bounds("theme-effects-background.window")
            .is_some()
    );
}
