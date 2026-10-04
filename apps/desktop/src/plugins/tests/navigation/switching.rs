use super::*;
use crate::plugins::navigation::Entry;
use sailry_protocol::Output;
use std::sync::Arc;

fn open(shell: &Entity<Shell>, entry: Entry, visual: &mut VisualTestContext) -> Entity<Panel> {
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.open_extension(entry, window, cx));
    });
    shell.read_with(visual, |shell, _| {
        shell.extensions.as_ref().unwrap().panel.clone().unwrap()
    })
}

#[track_caller]
fn unloaded(panel: &Entity<Panel>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        panel.read(cx).connected && panel.read(cx).metadata.read(cx).settled()
    });
    panel.read_with(visual, |panel, _| {
        assert!(panel.binding.worktree.is_none());
        assert!(panel.documents.is_none());
        assert!(!panel.loading);
        assert!(panel.mounted.is_none());
        assert!(panel.selected.is_none());
        assert!(panel.error.is_none());
    });
    assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
}

#[gpui::test]
fn empty_resources_stay_unloaded(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let node = runtime
            .block_on(sailry_node_runtime::Node::start(
                directory.path().join("node"),
            ))
            .unwrap();
        let controller = runtime
            .block_on(sailry_node_runtime::Node::start(
                directory.path().join("controller"),
            ))
            .unwrap();
        let address = runtime
            .block_on(
                controller
                    .link()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        let transport = if remote {
            controller.link().remote(address)
        } else {
            node.local()
        };
        let client = sailry_client::Client::new(transport);
        let Output::Snapshot(inventory) = runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected");
        };
        assert!(inventory.projects.is_empty());
        assert!(inventory.worktrees.is_empty());
        assert!(inventory.sessions.is_empty());
        let local = if remote { &controller } else { &node };
        cx.update(|cx| {
            cx.set_global(crate::backend::Services {
                runtime: runtime.clone(),
                local: local.local(),
                link: local.link(),
                relay_enabled: false,
            });
        });
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
                .contains_key(&node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(node.id(), cx)
            })
        });
        wait(visual, |cx| {
            let shell = shell.read(cx);
            let entries = shell.extension_entries(cx);
            ["files", "git"].into_iter().all(|name| {
                entries
                    .iter()
                    .any(|entry| entry.node == node.id() && entry.package.name == name)
            })
        });
        let entries: Vec<_> = shell.read_with(visual, |shell, cx| {
            ["files", "git"]
                .into_iter()
                .map(|name| {
                    shell
                        .extension_entries(cx)
                        .into_iter()
                        .find(|entry| entry.node == node.id() && entry.package.name == name)
                        .unwrap()
                })
                .collect()
        });
        for entry in &entries {
            assert!(entry.worktree.is_none());
            assert_eq!(
                entry.navigation.target,
                sailry_protocol::plugin::desktop::NavigationTarget::Worktree,
            );
        }
        let mut panels = Vec::new();
        for entry in &entries {
            let panel = open(&shell, entry.clone(), visual);
            unloaded(&panel, visual);
            panels.push(panel);
        }
        for index in [0, 1, 0, 1] {
            let cached = open(&shell, entries[index].clone(), visual);
            assert_eq!(cached, panels[index]);
            for panel in &panels {
                unloaded(panel, visual);
            }
            assert!(shell.read_with(visual, |shell, cx| {
                shell.needs_project() && !shell.has_resource_panel(cx)
            }));
        }
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(crate::preview::Page::Activity, window, cx)
            })
        });
        for index in [0, 1] {
            let cached = open(&shell, entries[index].clone(), visual);
            assert_eq!(cached, panels[index]);
            for panel in &panels {
                unloaded(panel, visual);
            }
        }
        for name in ["ssh", "databases", "progress"] {
            let entry = shell.read_with(visual, |shell, cx| {
                shell
                    .extension_entries(cx)
                    .into_iter()
                    .find(|entry| entry.node == node.id() && entry.package.name == name)
                    .unwrap()
            });
            assert!(entry.worktree.is_none());
            assert_eq!(
                entry.navigation.target,
                sailry_protocol::plugin::desktop::NavigationTarget::Node,
            );
            let panel = open(&shell, entry.clone(), visual);
            wait(visual, |cx| {
                panel.read(cx).resource_active() && !snapshot(&panel, cx).is_empty()
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.navigate(crate::preview::Page::Activity, window, cx)
                })
            });
            let cached = open(&shell, entry, visual);
            assert_eq!(cached, panel);
            wait(visual, |cx| panel.read(cx).resource_active());
            assert!(panel.read_with(visual, |panel, _| panel.binding.worktree.is_none()));
            assert!(!shell.read_with(visual, |shell, _| shell.needs_project()));
            visual.update(|window, cx| {
                assert!(
                    window.notifications(cx).is_empty(),
                    "unexpected {name} notification: {}",
                    crate::feedback::tests::summary(window, cx),
                );
            });
        }
        visual.update(|window, _| window.remove_window());
        drop(panels);
        drop(shell);
        runtime.block_on(node.shutdown()).unwrap();
        runtime.block_on(controller.shutdown()).unwrap();
    }
}

#[gpui::test]
fn entry_reads_again_without_replacing_cached_state(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        let root = fixture.directory.path().join("project/package");
        let path = root.join("plugin.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let desktop = &mut manifest["extensions"]["dev.sailry.platform"]["desktop"];
        desktop["navigation"] =
            serde_json::json!({"label":"Cached panel","icon":"reicon:files/file-text"});
        desktop["navigation_options"] = serde_json::json!({"pinned":true,"target":"node"});
        std::fs::write(path, manifest.to_string()).unwrap();
        std::fs::write(
            root.join("dev.sailry.platform/desktop/main.js"),
            include_str!("entry.js"),
        )
        .unwrap();
        let package = fixture.install(0);
        assert!(package.issues.is_empty(), "{:?}", package.issues);
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
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .extension_entries(cx)
                .iter()
                .any(|entry| entry.package == package.summary.reference())
        });
        let entry = shell.read_with(visual, |shell, cx| {
            shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package == package.summary.reference())
                .unwrap()
        });
        let panel = open(&shell, entry.clone(), visual);
        wait(visual, |cx| snapshot(&panel, cx).contains("entry-reads-1"));
        let root = panel.read_with(visual, |panel, _| {
            panel.mounted.as_ref().unwrap().root.entity_id()
        });
        click(visual, "entry-edit");
        wait(visual, |cx| snapshot(&panel, cx).contains("entry-draft-1"));
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.navigate(crate::preview::Page::Activity, window, cx)
            })
        });
        let cursor = panel.read_with(visual, |panel, _| {
            panel.snapshot.borrow().as_ref().unwrap().cursor
        });
        let cached = open(&shell, entry, visual);
        assert_eq!(cached, panel);
        assert_eq!(
            cached.read_with(visual, |panel, _| panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .entity_id()),
            root
        );
        wait(visual, |cx| snapshot(&panel, cx).contains("entry-reads-2"));
        assert!(panel.read_with(visual, |_, cx| {
            snapshot(&panel, cx).contains("entry-draft-1")
        }));
        assert_eq!(
            panel.read_with(visual, |panel, _| panel
                .snapshot
                .borrow()
                .as_ref()
                .unwrap()
                .cursor),
            cursor
        );
        visual.update(|window, _| window.remove_window());
        drop(cached);
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn preserves_games_and_releases_disabled_hidden_panels(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    let (cards, server) = fixture.game(true);
    let mut servers = vec![server];
    for (name, fields) in [
        ("city-trader", &["player_1_model", "player_2_model"][..]),
        ("poker", &["player_1_model"][..]),
        ("liars-dice", &["player_1_model", "player_2_model"][..]),
        ("gomoku", &["ai_model"][..]),
        ("reversi", &["ai_model"][..]),
        ("xiangqi", &["ai_model"][..]),
    ] {
        servers.push(fixture.game_plugin(name, fields).1);
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
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1500.), px(1000.)));
    wait(visual, |cx| entry_names(shell.read(cx), cx) == expected);
    let entries: Vec<_> = shell
        .read_with(visual, |shell, cx| shell.extension_entries(cx))
        .into_iter()
        .filter(|entry| {
            entry.scope == sailry_protocol::plugin::Scope::Desktop
                && [
                    "city-trader",
                    "doudizhu",
                    "gomoku",
                    "liars-dice",
                    "poker",
                    "reversi",
                    "xiangqi",
                ]
                .contains(&entry.package.name.as_str())
        })
        .collect();
    assert_eq!(entries.len(), 7);
    let cards_entry = entries
        .iter()
        .find(|entry| entry.package == cards.summary.reference())
        .unwrap();
    let panel = open(&shell, cards_entry.clone(), visual);
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("ddz-status-ready")
    });
    click(visual, "ddz-deal");
    wait(visual, |cx| snapshot(&panel, cx).contains("ddz-status-bid"));
    click(visual, "ddz-bid-3");
    wait(visual, |cx| {
        let tree = snapshot(&panel, cx);
        tree.contains("ddz-status-turn") && !tree.contains("ddz-dealing")
    });
    let tree = visual.update(|_, cx| snapshot(&panel, cx));
    let value: String = tree
        .rsplit("ddz-slot-")
        .next()
        .unwrap()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    click(visual, Box::leak(format!("card-{value}").into_boxed_str()));
    let selected = format!("ddz-selected-{value}");
    wait(visual, |cx| snapshot(&panel, cx).contains(&selected));
    let cards_id = panel.entity_id();
    drop(panel);

    let mut retained = Vec::new();
    for entry in &entries {
        let panel = open(&shell, entry.clone(), visual);
        wait(visual, |cx| {
            let tree = snapshot(&panel, cx);
            tree.contains("-status-ready") || tree.contains("ddz-status-turn")
        });
        let root = panel.read_with(visual, |panel, _| {
            panel.mounted.as_ref().unwrap().root.entity_id()
        });
        retained.push((entry.clone(), panel.downgrade(), root));
    }
    for (entry, previous, root) in retained.iter().rev() {
        let panel = open(&shell, entry.clone(), visual);
        assert_eq!(panel.entity_id(), previous.upgrade().unwrap().entity_id());
        assert_eq!(
            panel.read_with(visual, |panel, _| panel
                .mounted
                .as_ref()
                .unwrap()
                .root
                .entity_id()),
            *root
        );
    }
    let panel = open(&shell, cards_entry.clone(), visual);
    assert_eq!(panel.entity_id(), cards_id);
    assert!(
        visual
            .update(|_, cx| snapshot(&panel, cx))
            .contains(&selected)
    );
    assert!(
        servers
            .iter()
            .all(|server| server.requests.lock().unwrap().is_empty())
    );
    drop(panel);

    let (entry, previous, root) = retained
        .iter()
        .find(|(entry, _, _)| entry.package.name == "gomoku")
        .unwrap();
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin {
        name: entry.package.name.clone(),
    }) else {
        panic!("plugin expected");
    };
    let Output::Plugin(disabled) = fixture.execute(Command::SetPluginEnabled {
        name: entry.package.name.clone(),
        expected_revision: info.summary.revision,
        enabled: false,
    }) else {
        panic!("plugin expected");
    };
    wait(visual, |_| previous.upgrade().is_none());
    fixture.execute(Command::SetPluginEnabled {
        name: entry.package.name.clone(),
        expected_revision: disabled.summary.revision,
        enabled: true,
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .extension_entries(cx)
            .iter()
            .any(|current| current.package.name == entry.package.name)
    });
    let current = shell.read_with(visual, |shell, cx| {
        shell
            .extension_entries(cx)
            .into_iter()
            .find(|current| current.package.name == entry.package.name)
            .unwrap()
    });
    let panel = open(&shell, current, visual);
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("gomoku-status-ready")
    });
    assert_ne!(
        panel.read_with(visual, |panel, _| panel
            .mounted
            .as_ref()
            .unwrap()
            .root
            .entity_id()),
        *root
    );
    drop(panel);

    let (entry, previous, _) = retained
        .iter()
        .find(|(entry, _, _)| entry.package.name == "reversi")
        .unwrap();
    let Output::PluginSettings(mut settings) = fixture.execute(Command::ReadPluginSettings {
        package: entry.package.clone(),
    }) else {
        panic!("settings expected");
    };
    settings
        .values
        .insert("player_name".into(), "Updated player".into());
    let Output::PluginSettings(updated) = fixture.execute(Command::SavePluginSettings {
        package: settings.package,
        values: settings.values,
        secrets: Default::default(),
    }) else {
        panic!("settings expected");
    };
    wait(visual, |_| previous.upgrade().is_none());
    wait(visual, |cx| {
        shell
            .read(cx)
            .extension_entries(cx)
            .iter()
            .any(|entry| entry.package == updated.package)
    });
    let current = shell.read_with(visual, |shell, cx| {
        shell
            .extension_entries(cx)
            .into_iter()
            .find(|entry| entry.package == updated.package)
            .unwrap()
    });
    let panel = open(&shell, current, visual);
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("reversi-status-ready")
    });
    let root = panel.read_with(visual, |panel, _| {
        panel.mounted.as_ref().unwrap().root.entity_id()
    });
    let mut values = updated.values;
    values.insert("player_name".into(), "Current player".into());
    let Output::PluginSettings(updated) = fixture.execute(Command::SavePluginSettings {
        package: updated.package,
        values,
        secrets: Default::default(),
    }) else {
        panic!("settings expected");
    };
    wait(visual, |cx| {
        panel.read(cx).selected.as_ref() == Some(&updated.package)
            && panel
                .read(cx)
                .mounted
                .as_ref()
                .is_some_and(|mounted| mounted.root.entity_id() != root)
            && snapshot(&panel, cx).contains("reversi-status-ready")
    });
    let current = shell.read_with(visual, |shell, cx| {
        shell
            .extension_entries(cx)
            .into_iter()
            .find(|entry| entry.package == updated.package)
            .unwrap()
    });
    open(&shell, cards_entry.clone(), visual);
    let restored = open(&shell, current, visual);
    assert_eq!(restored.entity_id(), panel.entity_id());
    drop(restored);
    visual.update(|window, _| window.remove_window());
    drop(panel);
    drop(shell);
    visual.cx.update(|_| {});
    visual.run_until_parked();
    assert!(
        retained
            .iter()
            .all(|(_, panel, _)| panel.upgrade().is_none()),
        "closing the window must release all retained game panels"
    );
    fixture.close();
}
