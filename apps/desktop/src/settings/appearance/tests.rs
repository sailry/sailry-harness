use super::*;
use core::prelude::v1::test;
use std::time::{Duration, Instant};

struct Mounted(Entity<Workspace>);
impl Render for Mounted {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.executor().advance_clock(Duration::from_millis(250));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn wait(cx: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        settle(cx);
        if cx.update(|_, cx| ready(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "theme UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    settle(cx);
    let center = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(center, Modifiers::default());
    settle(cx);
}

#[gpui::test]
fn package_selection(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let source = theme::fixture::write(directory.path(), "tide");
    let mut manifest = theme::fixture::manifest("tide");
    manifest["light"]["theme"]["font.size"] = serde_json::json!(20);
    manifest["light"]["theme"]["colors"]["primary.background"] = serde_json::json!("#7c3aed");
    manifest["dark"]["theme"]["colors"]["primary.background"] = serde_json::json!("#c4b5fd");
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let preferences = directory.path().join("desktop/preferences.json");
    cx.update(|cx| {
        cx.set_global(crate::preferences::Preferences::open(preferences.clone()));
        gpui_kit::init(cx);
        theme::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        view.update(cx, |view, cx| {
            view.select(super::super::Section::Appearance, cx)
        });
        owner = Some(view.clone());
        let mounted = cx.new(|_| Mounted(view));
        Root::new(mounted, window, cx)
    });
    let owner = owner.unwrap();
    click(visual, "theme-import");
    assert!(visual.did_prompt_for_paths());
    visual.simulate_path_prompt_response(|options| {
        assert!(options.directories && !options.files);
        Some(vec![source.clone()])
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !visual.update(|window, cx| window.has_active_dialog(cx)) {
        settle(visual);
        assert!(Instant::now() < deadline, "theme preview deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    settle(visual);
    assert!(visual.debug_bounds("theme-import-confirm").is_some());
    assert!(!directory.path().join("desktop/themes/tide").exists());
    visual.update(|window, cx| window.close_dialog(cx));
    settle(visual);
    visual.update(|window, cx| {
        import::preview(
            theme::package::Loaded::read(&source, true).unwrap(),
            window,
            cx,
        )
    });
    click(visual, "theme-import-confirm");
    wait(visual, |cx| {
        cx.global::<theme::Catalog>().packages.len() == 2
    });
    assert!(
        directory
            .path()
            .join("desktop/themes/tide/theme.json")
            .exists()
    );
    visual.update(|_, cx| assert_eq!(cx.global::<theme::Catalog>().selected, 0));

    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [420., 960.] {
            let handle = visual.update(|window, cx| {
                theme::select(Some(mode), window, cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            settle(visual);
            settle(visual);
            let first = visual.debug_bounds("theme-card-0").unwrap();
            let second = visual.debug_bounds("theme-card-1").unwrap();
            assert!(first.right() < second.left() && second.right() <= px(width));
            assert!(visual.debug_bounds("theme-remove-0").is_none());
            click(visual, "theme-card-1");
            visual.update(|window, cx| {
                assert_eq!(cx.global::<theme::Catalog>().selected, 1);
                assert_eq!(cx.theme().mode, mode);
                assert_eq!(cx.theme().switch_thumb, cx.theme().background);
                assert_ne!(cx.theme().primary, cx.theme().primary_foreground);
                let emphasis = gpui_kit::component::theme::try_parse_color(if mode.is_dark() {
                    "#c4b5fd"
                } else {
                    "#7c3aed"
                })
                .unwrap();
                assert_eq!(cx.theme().primary, emphasis);
                assert_eq!(cx.theme().semantic_tokens().colors.primary, emphasis);
                assert_eq!(
                    cx.theme().font_size,
                    if mode.is_dark() {
                        px(cx.global::<theme::Catalog>().packages[0]
                            .dark
                            .font_size
                            .unwrap())
                    } else {
                        px(20.)
                    }
                );
                assert_eq!(
                    cx.theme().group_box,
                    gpui_kit::component::theme::try_parse_color(if mode.is_dark() {
                        "#243845"
                    } else {
                        "#e0eef5"
                    })
                    .unwrap()
                );
                // Another settings view observes the same local catalog.
                let other = cx.new(|cx| Workspace::new(window, cx));
                assert_eq!(other.read(cx).section, super::super::Section::General);
                assert_eq!(owner.read(cx).section, super::super::Section::Appearance);
            });
        }
    }
    visual.update(|window, cx| {
        theme::select(None, window, cx);
        assert!(theme::follows_system(cx));
        let saved = crate::preferences::Preferences::open(preferences.clone());
        assert_eq!(saved.data.appearance.package, "tide");
        assert_eq!(saved.data.appearance.mode, theme::Mode::System);
        cx.set_global(saved);
        let restored = theme::Catalog::new(cx);
        assert_eq!(restored.selected, 1);
        assert_eq!(restored.packages[1].images.len(), 2);
        cx.set_global(restored);
    });
    std::fs::write(source.join("theme.json"), b"invalid").unwrap();
    visual.update(|window, cx| import::load(source.clone(), window, cx));
    wait(visual, |cx| {
        cx.global::<theme::Catalog>().error == Some("appearance_import_invalid")
    });
    visual.update(|_, cx| assert_eq!(cx.global::<theme::Catalog>().selected, 1));
    click(visual, "theme-remove-1");
    assert!(directory.path().join("desktop/themes/tide").exists());
    crate::prompts::tests::answer(visual, "settings_delete");
    visual.update(|_, cx| {
        assert_eq!(cx.global::<theme::Catalog>().selected, 0);
        assert_eq!(cx.global::<theme::Catalog>().packages.len(), 1);
        let base = gpui_kit::component::theme::try_parse_color(if cx.theme().is_dark() {
            "blue-400"
        } else {
            "blue-600"
        })
        .unwrap();
        assert_eq!(cx.theme().primary, base);
    });
    assert!(!directory.path().join("desktop/themes/tide").exists());
    assert_eq!(
        crate::preferences::Preferences::open(preferences)
            .data
            .appearance
            .package,
        "builtin"
    );
}
