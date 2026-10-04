use super::*;
use crate::preview::Page;

#[gpui::test]
fn bundled_games_register_in_more(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    let games = [
        "city-trader",
        "doudizhu",
        "gomoku",
        "liars-dice",
        "poker",
        "reversi",
        "xiangqi",
    ];
    for name in games {
        fixture.execute(Command::InstallBundledPlugin {
            name: name.into(),
            expected_revision: 0,
        });
    }
    let expected: std::collections::BTreeSet<_> = [
        sailry_protocol::plugin::Scope::Host,
        sailry_protocol::plugin::Scope::Desktop,
    ]
    .into_iter()
    .flat_map(|scope| declared_navigation(&fixture, &fixture.binding.client, scope))
    .collect();
    cx.update(|cx| cx.set_global(fixture.services(false)));
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    wait(visual, |cx| entry_names(shell.read(cx), cx) == expected);
    shell.read_with(visual, |shell, cx| {
        for name in games {
            let entries = shell.extension_entries(cx);
            let entry = entries
                .iter()
                .find(|entry| entry.package.name == name)
                .unwrap();
            assert!(entry.icon.valid(), "{name}");
        }
    });
    click(visual, "navigation-more");
    let selector = format!("more-navigation-plugin-{:?}-city-trader", fixture.node.id());
    assert!(
        visual
            .debug_bounds(Box::leak(selector.into_boxed_str()))
            .is_some()
    );
    visual.simulate_keystrokes("escape");
    visual.update(|window, _| window.remove_window());
    drop(shell);
    fixture.close();
}

#[gpui::test]
fn management_uses_the_selected_host(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let installed = fixture.install(0);
        cx.update(|cx| cx.set_global(fixture.services(remote)));
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let shell = cx.new(|cx| Shell::new(window, cx));
            owner = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let shell = owner.unwrap();
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&fixture.node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
            });
        });
        wait(visual, |cx| {
            shell.read(cx).live.as_ref().unwrap().view.connected
                && shell.read(cx).extension_entries(cx).iter().any(|entry| {
                    entry.package.name == "databases" && entry.node == fixture.node.id()
                })
        });
        click(visual, "navigation-settings_plugins");
        assert_eq!(
            shell.read_with(visual, |shell, _| shell.page),
            Page::Plugins
        );
        assert!(visual.debug_bounds("shell-navigation").is_none());
        click(visual, "plugins-manage-tab");
        let selector: &'static str =
            Box::leak(format!("plugin-details-{}", installed.summary.name).into_boxed_str());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if visual.debug_bounds(selector).is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "plugin inventory deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(visual.debug_bounds("plugins-install").is_some());
        for (tab, control) in [
            ("plugins-skills-tab", "skills-install"),
            ("plugins-mcp-tab", "mcp-add"),
            ("plugins-manage-tab", selector),
        ] {
            click(visual, tab);
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds(control).is_some(), "{control}");
            assert_eq!(
                shell.read_with(visual, |shell, _| shell.page),
                Page::Plugins
            );
        }
        visual.update(|window, _| window.remove_window());
        drop(shell);
        fixture.close();
    }
}
