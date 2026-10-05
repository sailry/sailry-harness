use super::*;
use sailry_protocol::plugin::Origin;
use std::time::{Duration, Instant};

fn prompt(visual: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !visual.did_prompt_for_paths() {
        draw(visual);
        assert!(Instant::now() < deadline, "plugin path prompt deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn ready(
    owner: &Entity<Workspace>,
    visual: &mut VisualTestContext,
    name: &str,
    revision: u64,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(visual);
        let (current, settled, errors, action_error) = owner.read_with(visual, |owner, cx| {
            let catalog = &owner.plugin_catalog;
            let metadata = catalog.metadata.as_ref().unwrap().read(cx);
            (
                catalog
                    .packages
                    .iter()
                    .find(|package| package.name == name)
                    .cloned(),
                metadata.settled(),
                metadata.errors.clone(),
                catalog.error,
            )
        });
        if current
            .as_ref()
            .is_some_and(|package| package.revision == revision)
            && settled
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "plugin {name} revision {revision}: {current:?}, metadata errors {errors:?}, action error {action_error:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn hint(fixture: &Fixture, visual: &mut VisualTestContext) -> Option<std::path::PathBuf> {
    let package = fixture
        .plugins()
        .into_iter()
        .find(|package| package.name == "example")
        .unwrap();
    visual.update(|_, cx| {
        crate::preferences::plugins::directory(
            fixture.client.target(),
            &package.name,
            &package.digest,
            cx,
        )
    })
}

fn origin(fixture: &Fixture) -> Option<Origin> {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: "example".into(),
    }) else {
        panic!("plugin expected")
    };
    info.origin
}

#[gpui::test]
fn reuses_directory_after_reopening_preferences(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let preferences = fixture.directory.path().join("preferences.json");
        cx.update(|cx| cx.set_global(crate::preferences::Preferences::open(preferences.clone())));
        fixture.package("1.0.0");
        let path = fixture.directory.path().join("project/package");
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        select(visual, path.clone());
        ready(&owner, visual, "example", 1);
        wait(visual, |_| origin(&fixture) == Some(Origin::Directory));
        assert_eq!(hint(&fixture, visual), Some(path.canonicalize().unwrap()));
        for (version, revision) in [("2.0.0", 2), ("3.0.0", 3)] {
            visual.update(|_, cx| {
                cx.set_global(crate::preferences::Preferences::open(preferences.clone()))
            });
            fixture.package(version);
            menu(visual, "plugin-menu-example", 1);
            assert!(!visual.did_prompt_for_paths());
            ready(&owner, visual, "example", revision);
            assert_eq!(fixture.plugins()[0].version.as_deref(), Some(version));
            assert_eq!(origin(&fixture), Some(Origin::Directory));
            assert_eq!(hint(&fixture, visual), Some(path.canonicalize().unwrap()));
        }
        menu(visual, "plugin-menu-example", 2);
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        wait(visual, |_| fixture.plugins().is_empty());
        wait(visual, |cx| {
            crate::preferences::data(cx)
                .plugin_directories
                .as_ref()
                .is_some_and(|directories| directories.is_empty())
        });
        assert!(visual.update(|_, cx| {
            crate::preferences::data(cx)
                .plugin_directories
                .unwrap()
                .is_empty()
        }));
        fixture.close(visual);
    }
}

#[gpui::test]
fn chooses_a_missing_directory_and_reselects_archives(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let original = fixture.directory.path().join("project/package");
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        select(visual, original.clone());
        ready(&owner, visual, "example", 1);
        visual.update(|window, cx| window.clear_notifications(cx));
        crate::feedback::tests::settle(visual);
        assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
        let remembered = hint(&fixture, visual);
        let moved = fixture.directory.path().join("moved-package");
        std::fs::rename(&original, &moved).unwrap();
        let requests = fixture.transport.requests.lock().unwrap().len();
        menu(visual, "plugin-menu-example", 1);
        prompt(visual);
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);
        visual.simulate_path_prompt_response(|_| None);
        crate::feedback::tests::settle(visual);
        assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
        assert_eq!(hint(&fixture, visual), remembered);
        assert_eq!(fixture.plugins()[0].revision, 1);
        menu(visual, "plugin-menu-example", 1);
        prompt(visual);
        select(visual, moved.clone());
        ready(&owner, visual, "example", 2);
        assert_eq!(hint(&fixture, visual), Some(moved.canonicalize().unwrap()));

        fixture.package("2.0.0");
        tap(visual, "plugins-install");
        select(visual, fixture.archive());
        ready(&owner, visual, "example", 3);
        assert_eq!(origin(&fixture), Some(Origin::Archive));
        assert!(hint(&fixture, visual).is_none());
        for revision in [4, 5] {
            menu(visual, "plugin-menu-example", 1);
            assert!(visual.did_prompt_for_paths());
            assert_eq!(fixture.plugins()[0].revision, revision - 1);
            select(visual, fixture.archive());
            ready(&owner, visual, "example", revision);
            assert_eq!(origin(&fixture), Some(Origin::Archive));
            assert!(hint(&fixture, visual).is_none());
        }
        fixture.close(visual);
    }
}

#[gpui::test]
fn invalid_directory_does_not_open_a_picker(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let path = fixture.directory.path().join("project/package");
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        select(visual, path.clone());
        ready(&owner, visual, "example", 1);
        let remembered = hint(&fixture, visual);
        std::fs::write(path.join("plugin.json"), b"invalid manifest").unwrap();
        let requests = fixture.transport.requests.lock().unwrap().len();
        menu(visual, "plugin-menu-example", 1);
        shown(visual, "plugin-retry", true);
        assert!(!visual.did_prompt_for_paths());
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), requests);
        assert_eq!(fixture.plugins()[0].revision, 1);
        assert_eq!(hint(&fixture, visual), remembered);
        fixture.close(visual);
    }
}

#[gpui::test]
fn conflicting_selection_preserves_the_directory(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let path = fixture.directory.path().join("project/package");
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        select(visual, path);
        ready(&owner, visual, "example", 1);
        let remembered = hint(&fixture, visual);
        tap(visual, "plugins-install");
        fixture.execute(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: 1,
            enabled: false,
        });
        select(visual, fixture.archive());
        shown(visual, "plugin-retry", true);
        ready(&owner, visual, "example", 2);
        assert_eq!(origin(&fixture), Some(Origin::Directory));
        assert_eq!(hint(&fixture, visual), remembered);
        fixture.close(visual);
    }
}

#[gpui::test]
fn uses_the_execution_worktree(cx: &mut TestAppContext) {
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
        ready(&owner, visual, "example", 1);
        fixture.package("2.0.0");
        menu(visual, "plugin-menu-example", 1);
        assert!(!visual.did_prompt_for_paths());
        ready(&owner, visual, "example", 2);
        assert_eq!(fixture.plugins()[0].version.as_deref(), Some("2.0.0"));
        assert_eq!(
            origin(&fixture),
            Some(Origin::Worktree {
                worktree: fixture.worktree,
                path: "package".into()
            })
        );
        assert!(hint(&fixture, visual).is_none());
        fixture.close(visual);
    }
}

#[gpui::test]
fn keeps_installed_market_actions_disabled(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SetPluginEnabled {
            name: "files".into(),
            expected_revision: 1,
            enabled: false,
        });
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual, "files", 2);
        menu(visual, "plugin-menu-files", 1);
        assert!(
            !visual.has_pending_prompt(),
            "update opened a confirmation: {:?}",
            visual.pending_prompt()
        );
        assert!(!visual.did_prompt_for_paths());
        wait(visual, |cx| {
            !owner.read(cx).plugin_catalog.updates.checking()
        });
        ready(&owner, visual, "files", 2);
        tap(visual, "plugins-market-tab");
        shown(visual, "market-install-files", true);
        tap(visual, "market-install-files");
        for _ in 0..10 {
            draw(visual);
        }
        ready(&owner, visual, "files", 2);
        assert_eq!(
            fixture
                .public_packages(false)
                .iter()
                .find(|plugin| plugin.name == "files")
                .unwrap()
                .revision,
            2
        );
        assert!(!visual.did_prompt_for_paths());
        let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
            name: "files".into(),
        }) else {
            panic!("plugin expected")
        };
        assert_eq!(info.origin, Some(Origin::Bundled));
        assert!(!info.summary.enabled);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        fixture.close(visual);
    }
}

#[gpui::test]
fn checks_before_update_and_recovers_original_request(cx: &mut TestAppContext) {
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
        ready(&owner, visual, "example", 1);
        shown(visual, "plugins-update-all", false);
        fixture.package("2.0.0");
        tap(visual, "plugins-check-updates");
        shown(visual, "plugin-update-example", true);
        shown(visual, "plugins-update-all", true);
        assert_eq!(fixture.plugins()[0].revision, 1);
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "plugins-update-all");
        shown(visual, "plugins-retry", true);
        assert_eq!(fixture.plugins()[0].revision, 2);
        let first = fixture.transport.requests.lock().unwrap()[0];
        tap(visual, "plugins-retry");
        wait(visual, |cx| !owner.read(cx).plugin_catalog.busy());
        assert_eq!(*fixture.transport.requests.lock().unwrap(), [first, first]);
        ready(&owner, visual, "example", 2);
        shown(visual, "plugins-retry", false);
        tap(visual, "plugins-check-updates");
        wait(visual, |cx| {
            !owner.read(cx).plugin_catalog.updates.checking()
        });
        shown(visual, "plugins-update-all", false);
        shown(visual, "plugin-update-example", false);
        assert_eq!(fixture.plugins()[0].revision, 2);
        fixture.close(visual);
    }
}

#[gpui::test]
fn checks_remembered_directories_without_installing(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        select(visual, fixture.directory.path().join("project/package"));
        ready(&owner, visual, "example", 1);
        fixture.package("2.0.0");
        tap(visual, "plugins-check-updates");
        shown(visual, "plugin-update-example", true);
        assert_eq!(fixture.plugins()[0].revision, 1);
        assert!(!visual.did_prompt_for_paths());
        tap(visual, "plugin-update-example");
        ready(&owner, visual, "example", 2);
        assert_eq!(origin(&fixture), Some(Origin::Directory));
        assert_eq!(
            hint(&fixture, visual),
            Some(
                fixture
                    .directory
                    .path()
                    .join("project/package")
                    .canonicalize()
                    .unwrap()
            )
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn bulk_updates_continue_after_a_revision_conflict(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let second = fixture.directory.path().join("project/second");
        std::fs::create_dir(&second).unwrap();
        let write_second = |version: &str| {
            std::fs::write(
                second.join("plugin.json"),
                serde_json::json!({
                    "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                    "name": "second", "version": version,
                })
                .to_string(),
            )
            .unwrap();
        };
        fixture.package("1.0.0");
        write_second("1.0.0");
        for (path, name) in [("package", "example"), ("second", "second")] {
            fixture.execute(Command::InstallPlugin {
                worktree: fixture.worktree,
                path: path.into(),
                name: name.into(),
                expected_revision: 0,
            });
        }
        let (owner, visual) = fixture.mount(cx);
        ready(&owner, visual, "second", 1);
        fixture.package("2.0.0");
        write_second("2.0.0");
        tap(visual, "plugins-check-updates");
        wait(visual, |cx| {
            !owner.read(cx).plugin_catalog.updates.checking()
        });
        assert_eq!(
            owner.read_with(visual, |owner, _| owner.plugin_catalog.updates.available()),
            2
        );
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        // Change Node state before its subscription is reduced by the UI.
        fixture.execute(Command::SetPluginEnabled {
            name: "example".into(),
            expected_revision: 1,
            enabled: false,
        });
        visual.update(|window, cx| {
            owner.update(cx, |owner, cx| owner.update_plugins(None, window, cx))
        });
        wait(visual, |cx| !owner.read(cx).plugin_catalog.busy());
        ready(&owner, visual, "second", 2);
        let packages = fixture.plugins();
        let first = packages
            .iter()
            .find(|package| package.name == "example")
            .unwrap();
        assert_eq!(first.version.as_deref(), Some("1.0.0"));
        assert!(!first.enabled);
        assert_eq!(
            packages
                .iter()
                .find(|package| package.name == "second")
                .unwrap()
                .version
                .as_deref(),
            Some("2.0.0")
        );
        assert_eq!(fixture.transport.requests.lock().unwrap().len(), 2);
        assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
        fixture.close(visual);
    }
}
