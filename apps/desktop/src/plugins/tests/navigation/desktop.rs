use super::*;
use sailry_client::Client;
use sailry_protocol::{Output, plugin::Scope};

#[test]
fn desktop_packages_are_not_turn_defaults() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let queue = || {
            let Output::Session(session) = fixture.execute(Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            }) else {
                panic!("session expected");
            };
            let Output::QueuedTurn(turn) = fixture.execute(Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Inspect tools".into(),
            }) else {
                panic!("turn expected");
            };
            let mut packages = turn.plugins;
            packages.sort_by(|left, right| left.name.cmp(&right.name));
            packages
        };
        let baseline = queue();
        assert!(baseline.iter().any(|package| package.name == "reminders"));
        assert!(
            baseline
                .iter()
                .any(|package| package.name == "scheduled-tasks")
        );
        fixture.package();
        let path = fixture.directory.path().join("project/package/plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["extensions"]["dev.sailry.platform"]["scope"] = "desktop".into();
        std::fs::write(&path, manifest.to_string()).unwrap();
        let installed = fixture.install(0);
        assert_eq!(installed.extension.unwrap().scope, Scope::Desktop);
        assert!(
            !baseline
                .iter()
                .any(|package| package.name == installed.summary.name)
        );
        assert_eq!(queue(), baseline);
        manifest["extensions"]["dev.sailry.platform"]["scope"] = "host".into();
        std::fs::write(&path, manifest.to_string()).unwrap();
        let installed = fixture.install(installed.summary.revision);
        let mut expected = baseline;
        expected.push(installed.summary.reference());
        expected.sort_by(|left, right| left.name.cmp(&right.name));
        assert_eq!(queue(), expected);
        fixture.close();
    }
}

#[gpui::test]
fn host_selection_keeps_local_state(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(true);
    let (package, server) = fixture.game_plugin("gomoku", &["ai_model"]);
    let remote = Client::new(fixture.controller.local());
    let mut expected = declared_navigation(&fixture, &remote, Scope::Host);
    expected.extend(declared_navigation(
        &fixture,
        &fixture.binding.client,
        Scope::Desktop,
    ));
    cx.update(|cx| cx.set_global(fixture.services(false)));
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    wait(visual, |cx| {
        let shell = shell.read(cx);
        shell
            .live
            .as_ref()
            .unwrap()
            .hosts
            .contains_key(&fixture.controller.id())
            && shell
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package == package.summary.reference())
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let entry = shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package == package.summary.reference())
                .unwrap();
            shell.open_extension(entry, window, cx);
        })
    });
    let panel = shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    });
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("gomoku-status-ready")
    });
    let mounted = panel.read_with(visual, |panel, _| {
        panel.mounted.as_ref().unwrap().root.entity_id()
    });
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell
                .live
                .as_mut()
                .unwrap()
                .select(fixture.controller.id(), cx)
        });
    });
    wait(visual, |cx| {
        shell.read(cx).live.as_ref().unwrap().view.connected
            && entry_names(shell.read(cx), cx) == expected
    });
    assert_eq!(
        shell.read_with(visual, |shell, _| shell
            .extensions
            .as_ref()
            .unwrap()
            .panel
            .as_ref()
            .unwrap()
            .entity_id()),
        panel.entity_id()
    );
    assert_eq!(
        panel.read_with(visual, |panel, _| panel
            .mounted
            .as_ref()
            .unwrap()
            .root
            .entity_id()),
        mounted
    );
    assert_eq!(
        panel.read_with(visual, |panel, _| panel.binding.client.target()),
        fixture.node.id()
    );
    let entries = shell.read_with(visual, |shell, cx| shell.extension_entries(cx));
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.package.name.clone())
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.package.name == "gomoku")
            .unwrap()
            .node,
        fixture.node.id()
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.package.name == "reminders")
            .unwrap()
            .node,
        fixture.controller.id()
    );

    let Output::Providers(providers) = fixture
        .runtime
        .block_on(remote.execute(remote.prepare(Command::ListProviders)))
        .unwrap()
    else {
        panic!("providers expected")
    };
    assert!(providers.is_empty());
    // The shared SDK model catalog and configured choice still come from the local Node.
    let bounds = visual.debug_bounds("plugin-panel").unwrap();
    control(
        visual,
        f32::from(bounds.size.width) / 2. + 60.,
        f32::from(bounds.size.height) / 2. + 114.,
    );
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("gomoku-status-yourTurn")
    });
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    click(visual, "plugin-settings");
    // Games configure in their own modal while keeping the local game mounted.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.debug_bounds("plugin-setting-player_name").is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "game settings deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.page),
        crate::preview::Page::Plugin
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.settings_target),
        None
    );
    click(visual, "plugin-settings-cancel");
    assert!(
        visual
            .update(|_, cx| snapshot(&panel, cx))
            .contains("gomoku-status-yourTurn")
    );
    visual.update(|window, _| window.remove_window());
    drop(panel);
    drop(shell);
    fixture.close();
}

#[gpui::test]
fn routes_schedules_to_the_selected_host(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(true);
    let local = fixture.node.id();
    let remote = fixture.controller.id();
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: "scheduled-tasks".into(),
    }) else {
        panic!("plugin expected");
    };
    assert_eq!(info.extension.unwrap().scope, Scope::Host);
    cx.update(|cx| cx.set_global(fixture.services(false)));
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    let mut previous = None;
    for target in [local, remote, local, remote] {
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(target, cx)
            })
        });
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
                && shell.read(cx).extension_entries(cx).iter().any(|entry| {
                    entry.package.name == "scheduled-tasks"
                        && entry.node == target
                        && entry.scope == Scope::Host
                })
        });
        let entry = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "scheduled-tasks")
                .unwrap()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.open_extension(entry, window, cx))
        });
        let panel = shell.read_with(visual, |shell, _| {
            shell.extensions.as_ref().unwrap().panel.clone().unwrap()
        });
        wait(visual, |cx| {
            snapshot(&panel, cx).contains("scheduled-tasks-page")
        });
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.binding.client.target()),
            target
        );
        if let Some(previous) = previous {
            assert_ne!(panel.entity_id(), previous);
        }
        previous = Some(panel.entity_id());
        assert!(visual.debug_bounds("activity-host-filter").is_none());
    }
    let panel = shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    });
    let retained = panel.read_with(visual, |panel, _| {
        panel.mounted.as_ref().unwrap().root.entity_id()
    });
    let bounds = visual.debug_bounds("plugin-panel").unwrap();
    fixture
        .runtime
        .block_on(fixture.controller.shutdown())
        .unwrap();
    wait(visual, |cx| {
        !shell.read(cx).live.as_ref().unwrap().view.connected
    });
    wait(visual, |cx| {
        panel
            .read(cx)
            .mounted
            .as_ref()
            .is_some_and(|mounted| !mounted.active())
    });
    assert_eq!(visual.debug_bounds("plugin-panel"), Some(bounds));
    assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("scheduled-tasks-page")));
    assert_eq!(
        panel.read_with(visual, |panel, _| panel
            .mounted
            .as_ref()
            .unwrap()
            .root
            .entity_id()),
        retained
    );
    assert!(!panel.read_with(visual, |panel, _| panel.connected));
    assert!(shell.read_with(visual, |shell, cx| {
        shell.extension_entries(cx).iter().all(|entry| {
            entry.package.name != "scheduled-tasks"
                || !shell.command_navigation_available(entry, cx)
        })
    }));
    visual.update(|window, _| window.remove_window());
    drop(panel);
    drop(shell);
    fixture.runtime.block_on(fixture.node.shutdown()).unwrap();
}
