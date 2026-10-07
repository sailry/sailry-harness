use super::*;
use std::path::Path;

struct Harness;

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn mount<'a>(cx: &'a mut TestAppContext, directory: &Path) -> &'a mut VisualTestContext {
    cx.executor().allow_parking();
    cx.update(|cx| {
        cx.set_global(crate::preferences::Preferences::open(
            directory.join("desktop/preferences.json"),
        ));
        gpui_kit::init(cx);
        theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let mounted = cx.new(|_| Harness);
        Root::new(mounted, window, cx)
    });
    visual
}

fn scroll(visual: &mut VisualTestContext, delta: f32) {
    let dialog = visual.debug_bounds("dialog-0").unwrap();
    visual.simulate_event(ScrollWheelEvent {
        position: dialog.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
        touch_phase: TouchPhase::Moved,
        modifiers: Modifiers::default(),
    });
    settle(visual);
}

#[gpui::test]
fn arranges_details_responsively(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let source = theme::fixture::write(directory.path(), "tide");
    let visual = mount(cx, directory.path());
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for width in [420., 960.] {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(820.)));
            settle(visual);
            visual.update(|window, cx| {
                import::preview(
                    theme::package::Loaded::read(&source, true).unwrap(),
                    window,
                    cx,
                )
            });
            settle(visual);
            let dialog = visual.debug_bounds("dialog-0").unwrap();
            let light = visual.debug_bounds("theme-preview-light").unwrap();
            let dark = visual.debug_bounds("theme-preview-dark").unwrap();
            if width < 520. {
                assert!(dark.top() > light.bottom());
                assert_eq!(light.left(), dark.left());
            } else {
                assert!(light.right() < dark.left());
                assert_eq!(light.top(), dark.top());
            }
            let description = visual.debug_bounds("theme-preview-description").unwrap();
            assert!(description.top() > light.bottom().max(dark.bottom()));
            let heading = visual
                .debug_bounds("settings-heading-appearance_information")
                .unwrap();
            assert!(heading.top() > description.bottom());
            let author = visual.debug_bounds("theme-info-author").unwrap();
            let version = visual.debug_bounds("theme-info-version").unwrap();
            let identifier = visual.debug_bounds("theme-info-id").unwrap();
            assert!(author.top() > heading.bottom());
            assert!(version.top() > author.bottom());
            assert!(identifier.top() > version.bottom());
            assert!(visual.debug_bounds("theme-license-text").is_none());
            for selector in [
                "theme-preview-light",
                "theme-preview-dark",
                "theme-preview-description",
                "settings-surface-appearance_information",
                "theme-info-author-control",
                "theme-info-version-control",
                "theme-info-id-control",
                "theme-import-cancel",
                "theme-import-confirm",
            ] {
                let bounds = visual.debug_bounds(selector).unwrap();
                assert!(bounds.left() >= dialog.left(), "left overflow: {selector}");
                assert!(
                    bounds.right() <= dialog.right(),
                    "right overflow: {selector}"
                );
            }
            click(visual, "theme-import-cancel");
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
            assert_eq!(
                visual.update(|_, cx| cx.global::<theme::Catalog>().packages.len()),
                1
            );
            assert!(!directory.path().join("desktop/themes/tide").exists());
        }
    }
}

#[gpui::test]
fn scrolls_license_with_fixed_actions(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let source = theme::fixture::write(directory.path(), "tide");
    let mut manifest = theme::fixture::manifest("tide");
    manifest["license"]["author"] =
        serde_json::json!("A long author name with contributors ".repeat(7));
    manifest["license"]["text"] = serde_json::json!(
        "Permission is granted to use and distribute this original test artwork.\n\n".repeat(100)
    );
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let visual = mount(cx, directory.path());
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(700.), px(520.)));
    visual.update(|window, cx| {
        import::preview(
            theme::package::Loaded::read(&source, true).unwrap(),
            window,
            cx,
        )
    });
    settle(visual);
    let author = visual.debug_bounds("theme-info-author").unwrap();
    let control = visual.debug_bounds("theme-info-author-control").unwrap();
    let card = visual
        .debug_bounds("settings-surface-appearance_information")
        .unwrap();
    assert!(control.right() <= card.right());
    assert!(control.size.height > px(28.));
    assert!(author.size.height >= control.size.height);
    let footer = ["theme-import-cancel", "theme-import-confirm"]
        .map(|selector| visual.debug_bounds(selector).unwrap());
    let dialog = visual.debug_bounds("dialog-0").unwrap();
    for button in footer {
        assert!(button.bottom() <= dialog.bottom());
        assert!(button.right() <= dialog.right());
    }
    assert!(visual.debug_bounds("theme-license-text").is_none());
    scroll(visual, -2000.);
    click(visual, "theme-license-toggle");
    let before = visual.debug_bounds("theme-license-text").unwrap();
    assert!(before.size.height > dialog.size.height);
    scroll(visual, -2000.);
    let after = visual.debug_bounds("theme-license-text").unwrap();
    assert!(after.top() < before.top() - px(100.));
    assert_eq!(
        ["theme-import-cancel", "theme-import-confirm"]
            .map(|selector| visual.debug_bounds(selector).unwrap()),
        footer,
    );
    click(visual, "theme-import-cancel");
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(!directory.path().join("desktop/themes/tide").exists());
    assert_eq!(
        theme::package::Loaded::read(&source, true)
            .unwrap()
            .manifest
            .license
            .text,
        manifest["license"]["text"].as_str().unwrap(),
    );
}
