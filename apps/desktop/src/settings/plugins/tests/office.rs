use super::*;
use std::time::{Duration, Instant};

fn wait_installation(owner: &Entity<Workspace>, visual: &mut VisualTestContext, fixture: &Fixture) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(visual);
        let (package, busy, error, preparations, connected) =
            owner.read_with(visual, |owner, _| {
                let catalog = &owner.plugin_catalog;
                (
                    catalog
                        .packages
                        .iter()
                        .find(|package| package.name == "office")
                        .cloned(),
                    catalog.busy(),
                    catalog.error,
                    catalog.python.preparations.clone(),
                    owner
                        .provider_link
                        .as_ref()
                        .is_some_and(|live| live.connected),
                )
            });
        if package
            .as_ref()
            .is_some_and(|package| package.revision == 3)
        {
            return;
        }
        assert!(
            error.is_none() && Instant::now() < deadline,
            "Office installation deadline: package={package:?}, busy={busy}, error={error:?}, preparations={preparations:?}, connected={connected}, admitted_requests={}",
            fixture.transport.requests.lock().unwrap().len(),
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn manages_bundled_office_like_other_packages(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let installations = fixture
            .node
            .register_python_environment_fixture("office")
            .unwrap();
        let Output::Plugin(installed) = fixture.execute(Command::ReadPlugin {
            name: "office".into(),
        }) else {
            panic!("Office package expected");
        };
        assert!(installed.summary.enabled);
        assert_eq!(
            installed.origin,
            Some(sailry_protocol::plugin::Origin::Bundled)
        );
        fixture.execute(Command::RemovePlugin {
            name: "office".into(),
            expected_revision: installed.summary.revision,
        });
        let initial = fixture.public_packages(false);
        let (owner, visual) = fixture.mount(cx);
        tap(visual, "plugins-market-tab");
        input(visual, "plugins-search-input", "office");
        tap(visual, "plugins-search");
        shown(visual, "market-install-office", true);
        tap(visual, "market-install-office");
        wait_installation(&owner, visual, &fixture);
        tap(visual, "plugins-manage-tab");
        tap(visual, "plugin-details-office");
        for selector in [
            "plugin-skill-word",
            "plugin-skill-excel",
            "plugin-skill-powerpoint",
            "plugin-skill-pdf",
        ] {
            shown(visual, selector, true);
        }
        visual.simulate_keystrokes("escape");
        // Public packages use the ordinary enable switch and uninstall menu.
        for enabled in [false, true] {
            tap(visual, "plugin-toggle-office");
            wait(visual, |cx| {
                owner
                    .read(cx)
                    .plugin_catalog
                    .packages
                    .iter()
                    .any(|p| p.name == "office" && p.enabled == enabled)
            });
        }
        updates::ready(&owner, visual, "office", 5);
        menu(visual, "plugin-menu-office", 2);
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        wait(visual, |cx| {
            owner.read(cx).plugin_catalog.packages == initial
        });
        assert!(fixture.plugins().is_empty());
        assert_eq!(installations.load(Ordering::SeqCst), 0);
        fixture.close(visual);
    }
}
