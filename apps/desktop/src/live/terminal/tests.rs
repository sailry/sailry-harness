use super::*;
use core::prelude::v1::test;
use gpui_kit::component::Root;
use sailry_node_runtime::Node;
use std::time::{Duration, Instant};

#[track_caller]
fn wait(cx: &mut VisualTestContext, shell: &Entity<Shell>, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });
        if shell.read_with(cx, |shell, _| {
            shell.terminals.pending.is_empty()
                && shell
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| {
                        snapshot.node == shell.live.as_ref().unwrap().selected
                            && snapshot
                                .terminals
                                .iter()
                                .filter(|terminal| terminal.status == Status::Running)
                                .count()
                                == count
                            && snapshot
                                .terminals
                                .iter()
                                .filter(|terminal| terminal.status == Status::Closed)
                                .all(|terminal| {
                                    terminal.worktree.is_none_or(|tree| {
                                        !shell.plugin_panes.contains(Target::Terminal(
                                            snapshot.node,
                                            tree,
                                            terminal.id,
                                        ))
                                    })
                                })
                    })
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "terminal lifecycle deadline waiting for {count}: {}",
            shell.read_with(cx, |shell, cx| {
                let live = shell.live.as_ref().unwrap();
                format!(
                    "node={:?}, tree={:?}, page={:?}, active={:?}, pending={:?}, terminals={:?}",
                    live.selected,
                    live.selected_worktree().map(|tree| tree.id),
                    shell.page,
                    shell.splits.read(cx).active,
                    shell.terminals.pending,
                    live.view.snapshot.as_ref().map(|snapshot| {
                        snapshot
                            .terminals
                            .iter()
                            .map(|info| {
                                let pane = info.worktree.map(|tree| {
                                    let target = Target::Terminal(snapshot.node, tree, info.id);
                                    (
                                        shell.plugin_panes.contains(target),
                                        shell.plugin_panes.terminal(target, cx).is_some(),
                                    )
                                });
                                (info.id, info.status.clone(), pane)
                            })
                            .collect::<Vec<_>>()
                    }),
                )
            }),
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

#[track_caller]
fn wait_removed(cx: &mut VisualTestContext, shell: &Entity<Shell>, target: Target) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        if !shell.read_with(cx, |shell, _| shell.plugin_panes.contains(target)) {
            return;
        }
        assert!(Instant::now() < deadline, "terminal pane closure deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn local_page_lifecycle(cx: &mut TestAppContext) {
    lifecycle(cx, false);
}

#[gpui::test]
fn remote_page_lifecycle(cx: &mut TestAppContext) {
    lifecycle(cx, true);
}

fn lifecycle(cx: &mut TestAppContext, remote: bool) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let node = runtime
        .block_on(Node::start(directory.path().join("node")))
        .unwrap();
    let controller = remote.then(|| {
        let controller = runtime
            .block_on(Node::start(directory.path().join("controller")))
            .unwrap();
        runtime
            .block_on(
                controller
                    .link()
                    .pair(node.link().invite().unwrap().ticket()),
            )
            .unwrap();
        controller
    });
    let desktop = controller.as_ref().unwrap_or(&node);
    let transport = if remote {
        let address = runtime
            .block_on(desktop.link().peers())
            .unwrap()
            .into_iter()
            .find(|peer| *peer.id.as_bytes() == node.id().0)
            .unwrap();
        desktop.link().remote(address)
    } else {
        node.local()
    };
    let client = Client::new(transport);
    let Output::Project(project) = runtime
        .block_on(client.execute(client.prepare(Command::RegisterProject {
            name: "Terminal fixture".into(),
            path: directory.path().to_str().unwrap().into(),
        })))
        .unwrap()
    else {
        panic!("project expected")
    };
    #[cfg(unix)]
    runtime
        .block_on(client.execute(client.prepare(Command::SaveTerminalSettings(
            sailry_protocol::terminal::Settings {
                shell: "/bin/sh".into(),
                environment: [("HOME".into(), directory.path().to_str().unwrap().into())].into(),
                ..Default::default()
            },
        ))))
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(Services {
            runtime: runtime.clone(),
            local: desktop.local(),
            link: desktop.link(),
            relay_enabled: false,
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    if remote {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            if shell.read_with(visual, |shell, _| {
                shell.live.as_ref().unwrap().hosts.contains_key(&node.id())
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "remote discovery deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        shell.update(visual, |shell, cx| {
            shell.live.as_mut().unwrap().select(node.id(), cx)
        });
    }
    wait(visual, &shell, 0);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.live.as_mut().unwrap().project = Some(project.id);
            shell.navigate(Page::Project, window, cx);
        })
    });
    let Output::Snapshot(snapshot) = runtime
        .block_on(client.execute(client.prepare(Command::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let tree = snapshot
        .worktrees
        .iter()
        .find(|tree| tree.project == Some(project.id))
        .unwrap()
        .id;
    let appearance = visual.update(|_, cx| crate::theme::terminal(cx));
    let Output::Terminal(inventory) = runtime
        .block_on(client.execute(client.prepare(Command::CreateTerminal(
            sailry_protocol::terminal::Launch {
                worktree: tree,
                viewport: sailry_protocol::terminal::Viewport {
                    columns: 80,
                    rows: 24,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                appearance,
            },
        ))))
        .unwrap()
    else {
        panic!("terminal expected")
    };
    wait(visual, &shell, 1);
    let inventory_target = Target::Terminal(node.id(), tree, inventory.id);
    assert!(!shell.read_with(visual, |shell, _| {
        shell.plugin_panes.contains(inventory_target)
    }));
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.live_resource_action(
                &crate::live::menus::Dispatch {
                    node: node.id(),
                    target: crate::live::menus::Target::Terminal(inventory.id),
                    command: crate::live::menus::Command::Close,
                },
                window,
                cx,
            );
        })
    });
    wait(visual, &shell, 0);
    wait_removed(visual, &shell, inventory_target);
    assert!(!shell.read_with(visual, |shell, _| {
        shell.plugin_panes.contains(inventory_target)
    }));
    shell.update(visual, |shell, cx| {
        shell.live.as_mut().unwrap().select(node.id(), cx)
    });
    wait(visual, &shell, 0);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            assert_eq!(shell.live.as_ref().unwrap().selected, node.id());
            shell.live.as_mut().unwrap().project = Some(project.id);
            shell.open_live_terminals(window, cx);
        })
    });
    for count in 1..=2 {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.terminal_action(None, window, cx))
        });
        wait(visual, &shell, count);
    }
    let ids: Vec<_> = shell.read_with(visual, |shell, _| {
        shell
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .unwrap()
            .terminals
            .iter()
            .filter(|info| info.status == Status::Running)
            .map(|info| info.id)
            .collect()
    });
    assert_eq!(ids.len(), 2);
    for id in &ids {
        let loading = Box::leak(format!("live-terminal-{id}-loading").into_boxed_str());
        assert!(visual.debug_bounds(loading).is_none());
        let icon = visual
            .debug_bounds(Box::leak(
                format!("live-terminal-{id}-icon").into_boxed_str(),
            ))
            .unwrap();
        let label = visual
            .debug_bounds(Box::leak(
                format!("live-terminal-{id}-label").into_boxed_str(),
            ))
            .unwrap();
        assert!(icon.left() > label.right());
        assert!((icon.center().y - label.center().y).abs() < px(1.));
    }
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let info = shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .terminals
                .iter()
                .find(|info| info.id == ids[0])
                .unwrap()
                .clone();
            shell.reveal_terminal(Some(project.id), &info, window, cx);
        })
    });
    #[cfg(unix)]
    for (sequence, state) in [
        ("133;C", "idle"),
        ("9;4;3", "loading"),
        ("9;4;0", "idle"),
        ("9;4;1;50", "loading"),
        ("9;4;4", "waiting"),
        ("9;4;2", "failed"),
        ("9;4;0", "idle"),
        ("9;4;3", "loading"),
        ("633;C", "idle"),
    ] {
        let info = shell.read_with(visual, |shell, _| {
            shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .terminals
                .iter()
                .find(|info| info.id == ids[0])
                .unwrap()
                .clone()
        });
        runtime.block_on(async {
            client
                .execute(client.prepare(Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input: sailry_protocol::terminal::Input::Paste {
                        text: format!("printf '\\033]{sequence}\\007'"),
                    },
                }))
                .await
                .unwrap();
            client
                .execute(client.prepare(Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input: sailry_protocol::terminal::Input::Key {
                        event: sailry_protocol::terminal::KeyEvent {
                            key: sailry_protocol::terminal::Key::Enter,
                            action: sailry_protocol::terminal::Action::Press,
                            modifiers: Default::default(),
                            utf8: None,
                            unshifted_codepoint: None,
                        },
                    },
                }))
                .await
                .unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let indicator = if state == "loading" {
            "loading"
        } else {
            "icon"
        };
        let selector = Box::leak(format!("live-terminal-{}-{indicator}", info.id).into_boxed_str());
        let header = Box::leak(format!("pane-terminal-{}-{state}", info.id).into_boxed_str());
        loop {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let applied = shell.read_with(visual, |shell, _| {
                shell
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .terminals
                    .iter()
                    .find(|entry| entry.id == info.id)
                    .and_then(|entry| entry.activity)
                    .is_some_and(|report| {
                        report.sequence > info.activity.map_or(0, |previous| previous.sequence)
                    })
            });
            if applied
                && visual.debug_bounds(&*selector).is_some()
                && visual.debug_bounds(&*header).is_none()
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "terminal activity indicator deadline: {sequence} -> {state}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let hidden = if state == "loading" {
            "icon"
        } else {
            "loading"
        };
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("live-terminal-{}-{hidden}", info.id).into_boxed_str()
                ))
                .is_none()
        );
    }
    #[cfg(unix)]
    for (sequence, reported, expected) in [
        ("0;Shared title", "Shared title", "Shared title"),
        ("2;Updated title", "Updated title", "Updated title"),
        (
            "2;user@remote-host:/srv/project",
            "user@remote-host:/srv/project",
            "project",
        ),
    ] {
        let info = shell.read_with(visual, |shell, _| {
            shell
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .unwrap()
                .terminals
                .iter()
                .find(|info| info.id == ids[0])
                .unwrap()
                .clone()
        });
        runtime.block_on(async {
            client
                .execute(client.prepare(Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input: sailry_protocol::terminal::Input::Paste {
                        text: format!("printf '\\033]{sequence}\\007'; sleep 1"),
                    },
                }))
                .await
                .unwrap();
        });
        runtime
            .block_on(client.execute(client.prepare(Command::InputTerminal {
                terminal: info.id,
                revision: info.revision,
                input: sailry_protocol::terminal::Input::Key {
                    event: sailry_protocol::terminal::KeyEvent {
                        key: sailry_protocol::terminal::Key::Enter,
                        action: sailry_protocol::terminal::Action::Press,
                        modifiers: Default::default(),
                        utf8: None,
                        unshifted_codepoint: None,
                    },
                },
            })))
            .unwrap();
        let target = crate::panes::Target::Terminal(node.id(), info.worktree.unwrap(), info.id);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            if shell.read_with(visual, |shell, cx| {
                let snapshot = shell.live.as_ref().unwrap().view.snapshot.as_ref().unwrap();
                let info = snapshot
                    .terminals
                    .iter()
                    .find(|info| info.id == ids[0])
                    .unwrap();
                let caption = &shell.splits.read(cx).pane(target).unwrap().read(cx).title;
                info.title.as_deref() == Some(reported)
                    && caption.as_ref() == expected
                    && shell
                        .terminal_title(node.id(), info.worktree.unwrap(), info.id, cx)
                        .as_ref()
                        == expected
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "terminal title update deadline: {:?}",
                shell.read_with(visual, |shell, cx| (
                    shell
                        .live
                        .as_ref()
                        .unwrap()
                        .view
                        .snapshot
                        .as_ref()
                        .unwrap()
                        .terminals
                        .iter()
                        .find(|entry| entry.id == info.id)
                        .cloned(),
                    shell
                        .splits
                        .read(cx)
                        .pane(target)
                        .unwrap()
                        .read(cx)
                        .title
                        .clone()
                ))
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let target = shell.read_with(visual, |shell, _| {
        let info = shell
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .unwrap()
            .terminals
            .iter()
            .find(|info| info.id == ids[0])
            .unwrap();
        Target::Terminal(node.id(), info.worktree.unwrap(), info.id)
    });
    let original = shell.read_with(visual, |shell, _| {
        shell.plugin_panes.panel_id(target).unwrap()
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.navigate(Page::Project, window, cx))
    });
    wait(visual, &shell, 2);
    click(visual, "project-record-terminals");
    click(
        visual,
        Box::leak(format!("project-record-{}", ids[0]).into_boxed_str()),
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.page),
        Page::Terminal
    );
    assert_eq!(
        shell.read_with(visual, |shell, _| shell
            .plugin_panes
            .panel_id(target)
            .unwrap()),
        original
    );
    visual.simulate_keystrokes("secondary-w");
    wait(visual, &shell, 1);
    wait_removed(visual, &shell, target);
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.page),
        Page::Terminal
    );
    assert!(shell.read_with(visual, |shell, _| { !shell.plugin_panes.contains(target) }));
    visual.simulate_keystrokes("secondary-w");
    wait(visual, &shell, 0);
    wait_removed(visual, &shell, Target::Terminal(node.id(), tree, ids[1]));
    assert_eq!(
        shell.read_with(visual, |shell, _| shell.page),
        Page::Conversation
    );
    assert!(shell.read_with(visual, |shell, cx| !matches!(
        shell.splits.read(cx).active,
        Some(crate::panes::Target::Terminal(..))
    )));
    visual.update(|window, _| window.remove_window());
    if let Some(controller) = controller {
        runtime.block_on(controller.shutdown()).unwrap();
    }
    runtime.block_on(node.shutdown()).unwrap();
}
