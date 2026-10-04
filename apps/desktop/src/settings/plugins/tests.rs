use super::super::test_support::{
    Fixture, draw, fill, init, input, plugin_menu as menu, select, shown, shown_settings, tap, wait,
};
use super::*;
use core::prelude::v1::test;
use std::sync::atomic::Ordering;
#[path = "tests/computer.rs"]
mod computer;
#[path = "tests/details.rs"]
mod details_view;
#[path = "tests/inventory.rs"]
mod inventory;
#[path = "tests/market.rs"]
mod market;
#[path = "tests/metadata.rs"]
mod metadata_cache;
#[path = "tests/office.rs"]
mod office;
#[path = "tests/receipts.rs"]
mod receipts;
#[path = "tests/updates.rs"]
mod updates;

#[gpui::test]
fn selects_without_a_dialog(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        assert!(visual.did_prompt_for_paths());
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.simulate_path_prompt_response(|_| None);
        draw(visual);
        assert!(fixture.plugins().is_empty());
        tap(visual, "plugins-install");
        fill(visual, &fixture);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(fixture.plugins()[0].version.as_deref(), Some("1.0.0"));
        assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
        fixture.close(visual);
    }
}

#[gpui::test]
fn selects_directory(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        select(visual, fixture.directory.path().join("project/package"));
        wait(visual, |_| fixture.plugins().len() == 1);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        fixture.close(visual);
    }
}

#[gpui::test]
fn updates_and_removes(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let initial = fixture.public_packages(false);
        fixture.package("1.0.0");
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        fill(visual, &fixture);
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|package| package.name == "example")
        });
        fixture.package("2.0.0");
        updates::ready(&owner, visual, "example", 1);
        menu(visual, "plugin-menu-example", 1);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        select(visual, fixture.archive());
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|package| {
                    package.name == "example" && package.version.as_deref() == Some("2.0.0")
                })
        });
        assert_eq!(fixture.plugins()[0].revision, 2);
        assert!(
            visual
                .update(|_, cx| crate::preferences::plugins::directory(
                    fixture.client.target(),
                    "example",
                    &fixture.plugins()[0].digest,
                    cx
                ))
                .is_none()
        );
        updates::ready(&owner, visual, "example", 2);
        // A stale selection must not overwrite a concurrent update.
        menu(visual, "plugin-menu-example", 1);
        fixture.execute(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: 2,
            enabled: false,
        });
        select(visual, fixture.archive());
        shown(visual, "plugin-retry", true);
        assert_eq!(fixture.plugins()[0].revision, 3);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        menu(visual, "plugin-menu-example", 2);
        crate::prompts::tests::answer(visual, "settings_cancel");
        assert_eq!(fixture.plugins().len(), 1);
        menu(visual, "plugin-menu-example", 2);
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        wait(visual, |cx| {
            owner.read(cx).plugin_catalog.packages == initial
        });
        fixture.close(visual);
    }
}

#[gpui::test]
fn reloads_conflicting_details(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 0,
        });
        let (owner, visual) = fixture.mount(cx);
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .metadata
                .as_ref()
                .unwrap()
                .read(cx)
                .settled()
        });
        fixture.transport.read_mode.store(1, Ordering::SeqCst);
        tap(visual, "plugin-details-example");
        assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
        shown(visual, "plugin-details-error", false);
        tap(visual, "plugin-details-retry");
        shown(visual, "plugin-skill-analysis", true);
        visual.simulate_keystrokes("escape");
        shown(visual, "plugin-live-details", false);

        fixture.transport.read_mode.store(2, Ordering::SeqCst);
        tap(visual, "plugin-details-example");
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        fixture.execute(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: 1,
            enabled: false,
        });
        fixture.transport.release.cancel();
        assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
        shown(visual, "plugin-details-error", false);
        shown(visual, "plugin-skill-analysis", false);
        tap(visual, "plugin-details-retry");
        assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
        shown(visual, "plugin-details-error", false);
        visual.simulate_keystrokes("escape");
        shown(visual, "plugin-live-details", false);
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|package| package.name == "example" && package.revision == 2)
        });
        tap(visual, "plugin-details-example");
        shown(visual, "plugin-skill-analysis", true);
        fixture.close(visual);
    }
}

#[gpui::test]
fn recovers_bound_receipt(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        fixture.bind(&owner, visual, true);
        fixture.transport.mode.store(1, Ordering::SeqCst);
        select(visual, fixture.directory.path().join("project/package"));
        shown(visual, "plugin-retry", true);
        assert_eq!(fixture.plugins().len(), 1);
        let other = fixture.public_packages(true);
        assert_eq!(
            owner.read_with(visual, |owner, _| owner.plugin_catalog.packages.clone()),
            other
        );
        assert!(
            visual
                .update(|_, cx| crate::preferences::plugins::directory(
                    fixture.client.target(),
                    "example",
                    &fixture.plugins()[0].digest,
                    cx
                ))
                .is_none()
        );
        fixture.package("changed after receipt loss");
        tap(visual, "plugin-retry");
        shown(visual, "plugin-retry", false);
        assert_eq!(fixture.plugins()[0].version.as_deref(), Some("1.0.0"));
        assert_eq!(
            visual.update(|_, cx| crate::preferences::plugins::directory(
                fixture.client.target(),
                "example",
                &fixture.plugins()[0].digest,
                cx
            )),
            Some(
                fixture
                    .directory
                    .path()
                    .join("project/package")
                    .canonicalize()
                    .unwrap()
            )
        );
        assert!(
            visual
                .update(|_, cx| crate::preferences::plugins::directory(
                    fixture.other.id(),
                    "example",
                    &fixture.plugins()[0].digest,
                    cx
                ))
                .is_none()
        );
        let ids = fixture.transport.requests.lock().unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1]);
        drop(ids);
        fixture.close(visual);
    }
}

#[gpui::test]
fn invalid_package_feedback(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let (_, visual) = fixture.mount(cx);
        let invalid = fixture.directory.path().join("invalid.zip");
        std::fs::write(&invalid, b"not a ZIP").unwrap();
        tap(visual, "plugins-install");
        select(visual, invalid);
        shown(visual, "plugin-retry", true);
        assert!(fixture.plugins().is_empty());
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        tap(visual, "plugin-retry");
        select(visual, fixture.archive());
        wait(visual, |_| fixture.plugins().len() == 1);
        shown(visual, "plugin-retry", false);
        fixture.close(visual);
    }
}
