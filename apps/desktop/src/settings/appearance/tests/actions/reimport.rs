use super::*;
use gpui_kit::test::TestWindowExt as _;
use std::path::PathBuf;

fn replacement(directory: &Path, zip: bool) -> PathBuf {
    let source = theme::fixture::write(directory, "tide");
    let mut manifest = theme::fixture::manifest("tide");
    manifest["name"] = serde_json::json!("Updated Tide");
    // The ID, not the release version, determines replacement.
    manifest["package_version"] = serde_json::json!("0.0.1");
    for mode in ["light", "dark"] {
        manifest[mode]["theme"]["colors"]["primary.background"] = serde_json::json!("#f97316");
    }
    for asset in manifest["light"]["assets"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        asset["path"] = serde_json::json!("assets/replacement.png");
    }
    std::fs::rename(
        source.join("assets/light.png"),
        source.join("assets/replacement.png"),
    )
    .unwrap();
    std::fs::write(
        source.join("theme.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    if zip {
        let archive = directory.join("replacement.zip");
        theme::package::Loaded::read(&source, true)
            .unwrap()
            .export(&archive)
            .unwrap();
        archive
    } else {
        source
    }
}

fn preview(visual: &mut VisualTestContext, source: &Path) {
    visual.update(|window, cx| {
        import::preview(
            theme::package::Loaded::read(source, true).unwrap(),
            window,
            cx,
        )
    });
    settle(visual);
    visual.update(|window, _| {
        assert_eq!(
            window
                .find(SharedString::from("theme-import-confirm"))
                .label(),
            Some(tr("appearance_replace").as_ref())
        )
    });
}

#[gpui::test]
fn refreshes_active_theme_and_preserves_selection(cx: &mut TestAppContext) {
    for mode in [Some(ThemeMode::Light), Some(ThemeMode::Dark), None] {
        let directory = tempfile::tempdir().unwrap();
        install(directory.path(), "tide", None);
        let (_, visual) = mount(cx, directory.path());
        visual.update(|window, cx| {
            theme::select(mode, window, cx);
            menu::apply("tide", window, cx);
        });
        settle(visual);
        let saved = std::fs::read(directory.path().join("desktop/preferences.json")).unwrap();
        let colors = visual.update(|_, cx| (cx.theme().mode, cx.theme().primary));
        let installed = directory.path().join("desktop/themes/tide");
        let original = std::fs::read(installed.join("theme.json")).unwrap();
        let source = replacement(directory.path(), mode == Some(ThemeMode::Dark));
        preview(visual, &source);
        click(visual, "theme-import-cancel");
        assert_eq!(
            std::fs::read(installed.join("theme.json")).unwrap(),
            original
        );
        visual.update(|_, cx| assert_eq!(cx.theme().primary, colors.1));
        preview(visual, &source);
        click(visual, "theme-import-confirm");
        wait(visual, |cx| {
            cx.global::<theme::Catalog>().packages[1].name.as_ref() == "Updated Tide"
        });
        notice(visual, "appearance_import_done");
        visual.update(|window, cx| {
            assert!(!window.has_active_dialog(cx));
            let catalog = cx.global::<theme::Catalog>();
            assert_eq!(catalog.packages.len(), 2);
            assert_eq!(catalog.selected, 1);
            assert_eq!(catalog.packages[1].id, "tide");
            assert!(
                catalog.packages[1]
                    .images
                    .contains_key("assets/replacement.png")
            );
            assert!(!catalog.packages[1].images.contains_key("assets/light.png"));
            assert_eq!(cx.theme().mode, colors.0);
            assert_eq!(
                cx.theme().primary,
                gpui_kit::component::theme::try_parse_color("#f97316").unwrap()
            );
            let restored = theme::Catalog::new(cx);
            assert_eq!(restored.selected, 1);
            assert_eq!(restored.packages[1].name.as_ref(), "Updated Tide");
            assert_eq!(
                restored.packages[1]
                    .manifest
                    .as_ref()
                    .unwrap()
                    .package_version
                    .as_deref(),
                Some("0.0.1")
            );
        });
        assert_eq!(
            std::fs::read(directory.path().join("desktop/preferences.json")).unwrap(),
            saved
        );
        assert!(!installed.join("assets/light.png").exists());
        // Importing exactly the same package is also a replacement, not a duplicate.
        preview(visual, &source);
        click(visual, "theme-import-confirm");
        wait(visual, |_| installed.join("theme.json").exists());
        wait(visual, |cx| {
            cx.global::<theme::Catalog>().packages.len() == 2
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while visual.update(|window, cx| window.has_active_dialog(cx)) {
            settle(visual);
            assert!(Instant::now() < deadline, "theme replacement deadline");
        }
        visual.update(|window, _| window.remove_window());
    }
}

#[gpui::test]
fn replacing_inactive_theme_does_not_apply_it(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    install(directory.path(), "tide", None);
    install(directory.path(), "shore", None);
    let (_, visual) = mount(cx, directory.path());
    visual.update(|window, cx| menu::apply("shore", window, cx));
    settle(visual);
    let colors = visual.update(|_, cx| (cx.theme().mode, cx.theme().primary));
    let selected = visual.update(|_, cx| cx.global::<theme::Catalog>().selected);
    let source = replacement(directory.path(), true);
    preview(visual, &source);
    click(visual, "theme-import-confirm");
    wait(visual, |cx| {
        cx.global::<theme::Catalog>()
            .packages
            .iter()
            .any(|package| package.id == "tide" && package.name.as_ref() == "Updated Tide")
    });
    visual.update(|window, cx| {
        let catalog = cx.global::<theme::Catalog>();
        assert_eq!(catalog.packages.len(), 3);
        assert_eq!(catalog.selected, selected);
        assert_eq!(catalog.packages[catalog.selected].id, "shore");
        assert_eq!(crate::preferences::data(cx).appearance.package, "shore");
        assert_eq!((cx.theme().mode, cx.theme().primary), colors);
        window.remove_window();
    });
}

#[gpui::test]
fn failed_replacement_preserves_theme_and_can_retry(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    install(directory.path(), "tide", None);
    let (_, visual) = mount(cx, directory.path());
    visual.update(|window, cx| menu::apply("tide", window, cx));
    let colors = visual.update(|_, cx| cx.theme().primary);
    let installed = directory.path().join("desktop/themes/tide");
    let original = std::fs::read(installed.join("theme.json")).unwrap();
    let source = replacement(directory.path(), false);
    preview(visual, &source);
    let blocked = directory.path().join("blocked");
    std::fs::write(&blocked, b"not a directory").unwrap();
    let root = visual.update(|_, cx| {
        cx.update_global::<theme::Catalog, _>(|catalog, _| catalog.root.replace(blocked))
    });
    click(visual, "theme-import-confirm");
    notice(visual, "appearance_save_failed");
    visual.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        let catalog = cx.global::<theme::Catalog>();
        assert_eq!(catalog.packages.len(), 2);
        assert_eq!(catalog.selected, 1);
        assert_eq!(catalog.packages[1].name.as_ref(), "Tide");
        assert_eq!(cx.theme().primary, colors);
        cx.update_global::<theme::Catalog, _>(|catalog, _| catalog.root = root);
    });
    assert_eq!(
        std::fs::read(installed.join("theme.json")).unwrap(),
        original
    );
    click(visual, "theme-import-confirm");
    wait(visual, |cx| {
        cx.global::<theme::Catalog>().packages[1].name.as_ref() == "Updated Tide"
    });
    visual.update(|window, _| window.remove_window());
}
