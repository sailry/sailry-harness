use super::*;
use crate::{
    panes::{Extension, Target},
    resources::SideResource,
    shell::{Shell, session_scope::Key},
};
use sailry_protocol::plugin::Info as Package;

fn mount_shell<'a>(
    fixture: &Fixture,
    remote: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, &'a mut VisualTestContext) {
    cx.update(|cx| cx.set_global(fixture.services(remote)));
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
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .transport_for(fixture.node.id())
            .is_some()
    });
    (shell, visual)
}

fn open<'a>(
    fixture: &Fixture,
    package: &Package,
    remote: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, Entity<Panel>, &'a mut VisualTestContext) {
    cx.update(|cx| cx.set_global(crate::preferences::Preferences::default()));
    let (shell, visual) = mount_shell(fixture, remote, cx);
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.live.as_mut().unwrap().select(fixture.node.id(), cx);
            let source = shell.chat_view(
                fixture.binding.clone(),
                Some(fixture.session.clone()),
                window,
                cx,
            );
            shell
                .chats
                .views
                .insert((fixture.node.id(), fixture.session.id), source.clone());
            shell.activate_session(
                Key::Session(fixture.node.id(), fixture.session.id),
                window,
                cx,
            );
            shell.open_plugin_panel(source, window, cx);
        })
    });
    let panel = shell.read_with(visual, |shell, _| {
        let Some(SideResource::Plugin(panel)) = &shell.side_resource else {
            panic!("plugin panel expected")
        };
        panel.clone()
    });
    wait(visual, |cx| {
        panel.read(cx).ready_for(&package.summary.reference(), cx)
    });
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        })
    });
    wait(visual, |cx| snapshot(&panel, cx).contains("commands-page"));
    (shell, panel, visual)
}

fn create(
    fixture: &Fixture,
    shell: &Entity<Shell>,
    visual: &mut VisualTestContext,
) -> (Target, Entity<Panel>) {
    click(visual, "terminal-new");
    wait(visual, |cx| {
        shell.read(cx).plugin_panes.panels.values().any(|panel| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .and_then(|mounted| mounted.terminals.first())
                .is_some_and(|view| view.read(cx).info().is_some())
        })
    });
    shell.read_with(visual, |shell, cx| {
        let (target, panel) = shell.plugin_panes.panels.iter().next().unwrap();
        let state = panel.read(cx);
        assert_eq!(state.binding.client.target(), fixture.node.id());
        assert_eq!(state.binding.worktree, Some(fixture.session.worktree));
        assert_eq!(state.session, None);
        assert_eq!(state.selected.as_ref().unwrap().name, "commands");
        assert_eq!(shell.splits.read(cx).active, Some(*target));
        assert_eq!(
            shell
                .splits
                .read(cx)
                .pane(*target)
                .unwrap()
                .read(cx)
                .extension,
            Some(Extension {
                package: "commands".into(),
            })
        );
        let Target::Terminal(node, tree, id) = *target else {
            panic!("terminal pane expected")
        };
        assert_eq!(node, fixture.node.id());
        assert_eq!(tree, fixture.session.worktree);
        assert_eq!(
            state.pane,
            Some(crate::plugins::panes::Resource::Terminal(id))
        );
        (*target, panel.clone())
    })
}

fn id(target: Target) -> TerminalId {
    let Target::Terminal(_, _, id) = target else {
        panic!("terminal pane expected")
    };
    id
}

fn close_button(target: Target) -> &'static str {
    Box::leak(format!("pane-close-{target:?}").into_boxed_str())
}

fn native(panel: &Entity<Panel>, visual: &mut VisualTestContext) -> Entity<crate::terminal::View> {
    panel.read_with(visual, |panel, _| {
        panel.mounted.as_ref().unwrap().terminals.first().unwrap()
    })
}

fn split(shell: &Entity<Shell>, session: Target, terminal: Target, visual: &mut VisualTestContext) {
    use gpui_kit::component::dock::{DockPlacement, DropTarget, PanelId};
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.activate_pane(session, window, cx);
            shell.splits.update(cx, |splits, cx| {
                splits.focus(session, cx);
                let workspace = splits.workspace(session, cx).unwrap();
                let panel = PanelId::from(splits.pane(session).unwrap().entity_id());
                let node = splits
                    .active_area(cx)
                    .unwrap()
                    .read(cx)
                    .layout(DockPlacement::Center)
                    .unwrap()
                    .find_panel_node(panel)
                    .unwrap();
                assert!(splits.drop(
                    workspace,
                    terminal,
                    DropTarget::new(node, Some(gpui_kit::base::Placement::Right)),
                    window,
                    cx,
                ));
                assert_eq!(splits.workspace(terminal, cx), Some(workspace));
                assert_eq!(splits.parent(terminal), Some(session));
            });
            shell.activate_pane(terminal, window, cx);
        })
    });
    wait(visual, |cx| {
        let splits = shell.read(cx).splits.read(cx);
        splits.workspace(terminal, cx) == splits.workspace(session, cx)
            && splits.active == Some(terminal)
    });
}

fn wait_title(
    shell: &Entity<Shell>,
    panel: &Entity<Panel>,
    view: &Entity<crate::terminal::View>,
    target: Target,
    expected: &str,
    visual: &mut VisualTestContext,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        visual.run_until_parked();
        let (title, reported, error) = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            (
                shell
                    .read(cx)
                    .splits
                    .read(cx)
                    .pane(target)
                    .unwrap()
                    .read(cx)
                    .title
                    .clone(),
                view.read(cx).info().and_then(|info| info.title.clone()),
                panel.read(cx).error,
            )
        });
        if title.as_ref() == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "pane title deadline: pane={title:?}, native={reported:?}, error={error:?}, view={}",
            visual.update(|_, cx| snapshot(panel, cx)),
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn shares_workspace_and_preserves_process_on_detach(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        let package = install(&fixture);
        let (shell, launcher, visual) = open(&fixture, &package, remote, cx);
        let (terminal, panel) = create(&fixture, &shell, visual);
        let session = Target::Session(fixture.node.id(), fixture.session.id);
        let view = native(&panel, visual);
        visual.simulate_input("printf 'pane-%s\\n' preserved");
        visual.simulate_keystrokes("enter");
        wait(visual, |_| {
            content(&fixture, id(terminal)).contains("pane-preserved")
        });
        // Node preserves 512 Unicode characters; the SDK must not impose a byte limit.
        let title = "终端🙂".repeat(100);
        assert!(title.len() > 512);
        assert!(title.chars().count() <= 512);
        visual.simulate_input(&format!("printf '\\033]2;{title}\\007'; sleep 1"));
        visual.simulate_keystrokes("enter");
        wait_title(&shell, &panel, &view, terminal, &title, visual);
        assert!(visual.update(|_, cx| snapshot(&panel, cx).contains("terminal-content-running")));
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .transport_for(fixture.controller.id())
                .is_some()
        });
        visual.update(|_, cx| {
            // A native mount can still be awaiting the focus requested by navigation.
            panel.update(cx, |panel, _| panel.pane_focus = true);
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx)
            })
        });
        wait(visual, |cx| {
            let live = shell.read(cx).live.as_ref().unwrap();
            live.view.connected
                && live
                    .view
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.node == fixture.controller.id())
        });
        assert!(visual.debug_bounds("split-workspaces").is_none());
        assert!(!panel.read_with(visual, |panel, _| panel.pane_focus));
        assert_eq!(native(&panel, visual), view);
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
            })
        });
        wait(visual, |cx| {
            let live = shell.read(cx).live.as_ref().unwrap();
            live.view.connected
                && live
                    .view
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.node == fixture.node.id())
        });
        split(&shell, session, terminal, visual);
        click(visual, close_button(terminal));
        wait(visual, |cx| {
            let splits = shell.read(cx).splits.read(cx);
            splits.workspace(terminal, cx) != splits.workspace(session, cx)
        });
        assert_eq!(native(&panel, visual), view);
        assert!(
            terminals(&fixture)
                .iter()
                .any(|info| { info.id == id(terminal) && info.status == Status::Running })
        );
        click(
            visual,
            Box::leak(format!("live-terminal-{}", id(terminal)).into_boxed_str()),
        );
        wait(visual, |cx| {
            shell.read(cx).splits.read(cx).active == Some(terminal)
        });
        let Output::Snapshot(current) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        fixture.execute(Command::RemoveSession {
            session: fixture.session.id,
            expected_revision: current
                .sessions
                .iter()
                .find(|session| session.id == fixture.session.id)
                .unwrap()
                .revision,
        });
        visual.simulate_input("printf 'session-%s\\n' removed");
        visual.simulate_keystrokes("enter");
        wait(visual, |_| {
            content(&fixture, id(terminal)).contains("session-removed")
        });
        let released = view.downgrade();
        drop(view);
        fixture.execute(Command::SetPluginEnabled {
            name: "commands".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            panel.read(cx).mounted.is_none() && released.upgrade().is_none()
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| shell.activate_pane(terminal, window, cx))
        });
        shell.read_with(visual, |shell, cx| {
            assert!(shell.splits.read(cx).contains(terminal));
        });
        assert!(
            terminals(&fixture)
                .iter()
                .any(|info| { info.id == id(terminal) && info.status == Status::Running })
        );
        fixture.execute(Command::SetPluginEnabled {
            name: "commands".into(),
            expected_revision: package.summary.revision + 1,
            enabled: true,
        });
        wait(visual, |cx| {
            panel.read(cx).mounted.as_ref().is_some_and(|mounted| {
                mounted.terminals.first().is_some_and(|view| {
                    view.read(cx)
                        .info()
                        .is_some_and(|info| info.id == id(terminal))
                })
            })
        });
        assert!(content(&fixture, id(terminal)).contains("pane-preserved"));
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| {
                    matches!(
                        request.command,
                        Command::OpenTerminal { .. } | Command::CloseTerminal { .. }
                    )
                })
        );
        click(visual, close_button(terminal));
        wait(visual, |cx| {
            !shell.read(cx).splits.read(cx).contains(terminal)
                && !shell.read(cx).plugin_panes.panels.contains_key(&terminal)
        });
        assert!(
            terminals(&fixture)
                .iter()
                .any(|info| { info.id == id(terminal) && info.status == Status::Closed })
        );
        let requests = fixture.transport.requests.lock().unwrap();
        let closes: Vec<_> = requests.iter().filter(|request| {
            matches!(request.command, Command::CloseTerminal { terminal: current, .. } if current == id(terminal))
        }).collect();
        assert_eq!(closes.len(), 1);
        let context = closes[0].plugin.as_ref().expect("scoped close expected");
        assert_eq!(context.package.name, "commands");
        assert_eq!(context.worktree, Some(fixture.session.worktree));
        assert_eq!(context.session, None);
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(launcher);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn disabling_releases_view(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        let package = install(&fixture);
        let (shell, launcher, visual) = open(&fixture, &package, remote, cx);
        let (terminal, panel) = create(&fixture, &shell, visual);
        fixture.transport.mode.store(11, Ordering::SeqCst);
        click(visual, close_button(terminal));
        wait(visual, |cx| {
            fixture.transport.entered.is_cancelled()
                && shell
                    .read(cx)
                    .splits
                    .read(cx)
                    .pane(terminal)
                    .unwrap()
                    .read(cx)
                    .closing
        });
        fixture.execute(Command::SetPluginEnabled {
            name: "commands".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            panel.read(cx).mounted.is_none()
                && !shell
                    .read(cx)
                    .splits
                    .read(cx)
                    .pane(terminal)
                    .unwrap()
                    .read(cx)
                    .closing
        });
        fixture.transport.release.cancel();
        assert!(
            terminals(&fixture)
                .iter()
                .any(|info| { info.id == id(terminal) && info.status == Status::Running })
        );
        click(visual, close_button(terminal));
        wait(visual, |cx| {
            !shell.read(cx).splits.read(cx).contains(terminal)
                && !shell.read(cx).plugin_panes.panels.contains_key(&terminal)
        });
        assert!(
            terminals(&fixture)
                .iter()
                .any(|info| { info.id == id(terminal) && info.status == Status::Running })
        );
        let requests = fixture.transport.requests.lock().unwrap();
        let closes: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::CloseTerminal { .. }))
            .collect();
        assert_eq!(closes.len(), 1);
        assert!(closes[0].plugin.is_some());
        assert!(
            !requests
                .iter()
                .any(|request| matches!(request.command, Command::OpenTerminal { .. }))
        );
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(launcher);
        drop(shell);
        fixture.close();
    }
}

#[gpui::test]
fn restores_package_binding_and_shell_process(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        let package = install(&fixture);
        let (shell, launcher, visual) = open(&fixture, &package, remote, cx);
        let (terminal, panel) = create(&fixture, &shell, visual);
        let session = Target::Session(fixture.node.id(), fixture.session.id);
        split(&shell, session, terminal, visual);
        let encoded = shell.read_with(visual, |shell, cx| {
            let saved = shell.splits.read(cx).saved(cx);
            saved.validate().unwrap();
            serde_json::to_string(&saved).unwrap()
        });
        let saved: crate::panes::Saved = serde_json::from_str(&encoded).unwrap();
        saved.validate().unwrap();
        assert_eq!(saved.active, Some(terminal));
        assert_eq!(saved.extensions.len(), 1);
        assert_eq!(saved.extensions[0].0, terminal);
        assert_eq!(saved.extensions[0].1.package, "commands");
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(launcher);
        drop(shell);
        visual.run_until_parked();

        let Fixture {
            directory,
            runtime,
            node,
            controller,
            transport,
            mut binding,
            session: record,
        } = fixture;
        runtime.block_on(node.shutdown()).unwrap();
        let node = runtime
            .block_on(sailry_node_runtime::Node::start(
                directory.path().join("node"),
            ))
            .unwrap();
        let connection = if remote {
            let address = runtime
                .block_on(
                    controller
                        .link()
                        .pair(node.link().invite().unwrap().ticket()),
                )
                .unwrap();
            controller.link().remote(address)
        } else {
            node.local()
        };
        binding.client = std::sync::Arc::new(sailry_client::Client::new(connection));
        binding.defaults = std::sync::Arc::new(sailry_client::Client::new(if remote {
            controller.local()
        } else {
            node.local()
        }));
        let fixture = Fixture {
            directory,
            runtime,
            node,
            controller,
            transport,
            binding,
            session: record,
        };
        assert!(
            terminals(&fixture)
                .iter()
                .any(|info| { info.id == id(terminal) && info.status == Status::Stopped })
        );
        cx.update(|cx| {
            crate::preferences::update(cx, |preferences| {
                preferences.workspaces = Some(serde_json::to_value(saved).unwrap());
            });
        });
        let (restored, visual) = mount_shell(&fixture, remote, cx);
        wait(visual, |cx| {
            let shell = restored.read(cx);
            shell
                .plugin_panes
                .panels
                .get(&terminal)
                .is_some_and(|panel| {
                    snapshot(panel, cx).contains("terminal-content-running")
                        && panel
                            .read(cx)
                            .mounted
                            .as_ref()
                            .and_then(|mounted| mounted.terminals.first())
                            .is_some_and(|view| {
                                view.read(cx).info().is_some_and(|info| {
                                    info.id == id(terminal) && info.status == Status::Running
                                })
                            })
                })
                && shell
                    .chats
                    .views
                    .get(&(fixture.node.id(), fixture.session.id))
                    .is_some_and(|view| view.read(cx).connected())
        });
        let panel = restored.read_with(visual, |shell, cx| {
            let splits = shell.splits.read(cx);
            assert_eq!(splits.active, Some(terminal));
            assert_eq!(
                splits.workspace(terminal, cx),
                splits.workspace(session, cx)
            );
            assert_eq!(splits.parent(terminal), Some(session));
            let panel = shell.plugin_panes.panels[&terminal].clone();
            let state = panel.read(cx);
            assert_eq!(state.binding.client.target(), fixture.node.id());
            assert_eq!(state.binding.worktree, Some(fixture.session.worktree));
            assert_eq!(state.session, None);
            assert_eq!(state.selected.as_ref().unwrap().name, "commands");
            assert_eq!(
                state.pane,
                Some(crate::plugins::panes::Resource::Terminal(id(terminal)))
            );
            let native = state.mounted.as_ref().unwrap().terminals.first().unwrap();
            let info = native.read(cx).info().unwrap();
            assert_eq!(info.id, id(terminal));
            assert_eq!(info.worktree, Some(fixture.session.worktree));
            assert_eq!(info.status, Status::Running);
            panel
        });
        assert!(
            terminals(&fixture)
                .iter()
                .any(|info| { info.id == id(terminal) && info.status == Status::Running })
        );
        assert!(visual.debug_bounds("terminal-resume").is_none());
        assert_eq!(terminals(&fixture).len(), 1);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(restored);
        fixture.close();
    }
}
