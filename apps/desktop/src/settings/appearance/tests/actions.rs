use super::*;
use std::path::Path;

mod reimport;

fn install(directory: &Path, id: &str, homepage: Option<&str>) {
    let source = theme::fixture::write(directory, id);
    if let Some(homepage) = homepage {
        let mut manifest = theme::fixture::manifest(id);
        manifest["homepage"] = serde_json::json!(homepage);
        std::fs::write(
            source.join("theme.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }
    theme::package::Loaded::read(&source, true)
        .unwrap()
        .install(&directory.join("desktop/themes"))
        .unwrap();
}

fn mount<'a>(
    cx: &'a mut TestAppContext,
    directory: &Path,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        cx.set_global(crate::preferences::Preferences::open(
            directory.join("desktop/preferences.json"),
        ));
        gpui_kit::init(cx);
        theme::init(cx);
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        view.update(cx, |view, cx| {
            view.select(super::super::super::Section::Appearance, cx)
        });
        owner = Some(view.clone());
        let mounted = cx.new(|_| Mounted(view));
        Root::new(mounted, window, cx)
    });
    settle(visual);
    (owner.unwrap(), visual)
}

fn command(
    visual: &mut VisualTestContext,
    owner: &Entity<Workspace>,
    id: &str,
    command: menu::Command,
) {
    dispatch(
        visual,
        owner,
        Box::new(menu::Dispatch {
            id: id.into(),
            command,
        }),
    );
}

fn notice(visual: &mut VisualTestContext, key: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        settle(visual);
        if visual.update(|window, cx| crate::feedback::tests::count(window, &tr(key), cx) > 0) {
            return;
        }
        assert!(Instant::now() < deadline, "theme feedback deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn captures_identity_and_capabilities(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    install(directory.path(), "tide", Some("https://example.com/tide"));
    install(directory.path(), "shore", None);
    let (owner, visual) = mount(cx, directory.path());
    let captured = visual.update(|_, cx| {
        let builtin = menu::definition("builtin", cx);
        assert_eq!(builtin.items.len(), 5);
        assert!(builtin.items[0].is_checked());
        assert!(!builtin.items[0].is_disabled());
        assert!(builtin.items[1..].iter().all(MenuItem::is_disabled));
        let shore = menu::definition("shore", cx);
        assert!(shore.items[2].is_disabled());
        let mut tide = menu::definition("tide", cx);
        assert!(!tide.items[0].is_checked());
        assert!(tide.items.iter().all(|item| !item.is_disabled()));
        assert!(menu::definition("missing", cx).items.is_empty());
        let MenuItem::Action { action, .. } = tide.items.remove(0) else {
            panic!("theme action expected")
        };
        action
    });
    visual.update(|_, cx| {
        cx.update_global::<theme::Catalog, _>(|catalog, _| catalog.packages.swap(1, 2));
    });
    dispatch(visual, &owner, captured);
    visual.update(|_, cx| {
        let catalog = cx.global::<theme::Catalog>();
        assert_eq!(catalog.packages[catalog.selected].id, "tide");
        assert!(menu::definition("tide", cx).items[0].is_checked());
        assert!(!menu::definition("shore", cx).items[0].is_checked());
        assert_eq!(crate::preferences::data(cx).appearance.package, "tide");
    });
    command(visual, &owner, "tide", menu::Command::Homepage);
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("https://example.com/tide")
    );
    let saved = std::fs::read(directory.path().join("desktop/preferences.json")).unwrap();
    command(visual, &owner, "shore", menu::Command::Details);
    assert!(visual.debug_bounds("theme-preview-details").is_some());
    assert!(visual.debug_bounds("theme-info-author").is_some());
    assert!(visual.debug_bounds("theme-import-confirm").is_none());
    click(visual, "theme-import-cancel");
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(
        std::fs::read(directory.path().join("desktop/preferences.json")).unwrap(),
        saved
    );
    visual.update(|_, cx| {
        let catalog = cx.global::<theme::Catalog>();
        assert_eq!(catalog.packages.len(), 3);
        assert_eq!(catalog.packages[catalog.selected].id, "tide");
    });
    command(visual, &owner, "builtin", menu::Command::Uninstall);
    assert!(visual.pending_prompt().is_none());
    command(visual, &owner, "tide", menu::Command::Uninstall);
    let (_, detail) = crate::prompts::tests::wait(visual);
    assert!(detail.contains("Tide"));
    crate::prompts::tests::answer(visual, "settings_cancel");
    assert!(directory.path().join("desktop/themes/tide").exists());
    command(visual, &owner, "tide", menu::Command::Uninstall);
    crate::prompts::tests::answer(visual, "appearance_uninstall");
    wait(visual, |cx| {
        cx.global::<theme::Catalog>().packages.len() == 2
    });
    assert!(!directory.path().join("desktop/themes/tide").exists());
    visual.update(|_, cx| {
        let catalog = cx.global::<theme::Catalog>();
        assert_eq!(catalog.packages[catalog.selected].id, "builtin");
        assert!(catalog.packages.iter().any(|package| package.id == "shore"));
    });
    notice(visual, "appearance_removed");
    command(visual, &owner, "tide", menu::Command::Apply);
    notice(visual, "appearance_unavailable");
    visual.update(|_, cx| assert_eq!(cx.global::<theme::Catalog>().selected, 0));
}

#[gpui::test]
fn exports_zip_and_imports_round_trip(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    install(directory.path(), "tide", Some("https://example.com/tide"));
    let (owner, visual) = mount(cx, directory.path());
    let destination = directory.path().join("tide.zip");
    command(visual, &owner, "tide", menu::Command::Export);
    assert!(visual.did_prompt_for_new_path());
    visual.simulate_new_path_selection(|_| None);
    settle(visual);
    assert!(!destination.exists());
    assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
    command(visual, &owner, "tide", menu::Command::Export);
    assert!(visual.did_prompt_for_new_path());
    visual.simulate_new_path_selection(|_| Some(directory.path().join("missing/theme.zip")));
    notice(visual, "appearance_export_failed");
    assert!(!destination.exists());
    command(visual, &owner, "tide", menu::Command::Export);
    visual.simulate_new_path_selection(|_| Some(destination.clone()));
    notice(visual, "appearance_export_done");
    let exported = theme::package::Loaded::read(&destination, true).unwrap();
    assert_eq!(exported.manifest.id, "tide");
    assert_eq!(
        exported.manifest.homepage.as_deref(),
        Some("https://example.com/tide")
    );
    assert_eq!(exported.images.len(), 2);
    visual.update(|window, cx| {
        assert_eq!(cx.global::<theme::Catalog>().selected, 0);
        assert_eq!(cx.global::<theme::Catalog>().packages.len(), 2);
        window.remove_window();
    });
    let imported = tempfile::tempdir().unwrap();
    let (_, visual) = mount(cx, imported.path());
    click(visual, "theme-import");
    assert!(visual.did_prompt_for_paths());
    visual.simulate_path_prompt_response(|options| {
        assert!(options.files && options.directories && !options.multiple);
        Some(vec![destination.clone()])
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !visual.update(|window, cx| window.has_active_dialog(cx)) {
        settle(visual);
        assert!(Instant::now() < deadline, "ZIP import preview deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    click(visual, "theme-import-confirm");
    wait(visual, |cx| {
        cx.global::<theme::Catalog>().packages.len() == 2
    });
    let restored =
        theme::package::Loaded::read(&imported.path().join("desktop/themes/tide"), true).unwrap();
    assert_eq!(restored.manifest.homepage, exported.manifest.homepage);
    assert_eq!(
        restored.manifest.license.text,
        exported.manifest.license.text
    );
    assert_eq!(restored.images.len(), exported.images.len());
}
