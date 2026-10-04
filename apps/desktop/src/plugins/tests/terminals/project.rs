use super::*;
use crate::{
    panes::{Extension, Target},
    plugins::contributions::{Entry, Registry},
    preview::Page,
    shell::Shell,
};
use sailry_protocol::{Request, plugin::ui::Slot, terminal::Tool};
use std::os::unix::fs::PermissionsExt;

fn programs(fixture: &Fixture) {
    let programs = fixture.directory.path().join("programs");
    std::fs::create_dir(&programs).unwrap();
    let program = programs.join("claude");
    std::fs::write(
        &program,
        concat!(
            "#!/bin/sh\n",
            "printf 'started\\n' >> starts.txt\n",
            "printf 'Project CLI ready\\n'\n",
            "while IFS= read -r line; do printf 'Fixture: %s\\n' \"$line\"; done\n",
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    fixture.execute(Command::SaveTerminalSettings(
        sailry_protocol::terminal::Settings {
            revision: 0,
            shell: String::new(),
            environment: [("PATH".into(), programs.to_str().unwrap().into())].into(),
        },
    ));
}

fn mount<'a>(
    fixture: &Fixture,
    remote: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, Entity<Registry>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        let mut services = fixture.services(remote);
        if !remote {
            services.local = fixture.transport.clone();
        }
        cx.set_global(services);
        cx.set_global(crate::preferences::Preferences::default());
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| {
            let mut shell = Shell::new(window, cx);
            shell
                .live
                .as_mut()
                .unwrap()
                .override_transport(fixture.transport.clone());
            shell
        });
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    wait(visual, |cx| {
        let live = shell.read(cx).live.as_ref().unwrap();
        live.transport_for(fixture.node.id()).is_some()
            && live.transport_for(fixture.controller.id()).is_some()
    });
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            let live = shell.live.as_mut().unwrap();
            live.select(fixture.node.id(), cx);
        });
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
    let registry = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let project = fixture.binding.project.unwrap();
            let live = shell.live.as_mut().unwrap();
            live.project = Some(project);
            shell.page = Page::Project;
            let registry = shell.project_plugins(project, window, cx).unwrap();
            cx.notify();
            registry
        })
    });
    wait(visual, |cx| {
        let entries = registry.read(cx).entries(Slot::Project, cx);
        entries.len() == 2 && entries.iter().all(|entry| entry.state.enabled)
    });
    (shell, registry, visual)
}

fn entry(registry: &Entity<Registry>, id: &str, cx: &App) -> Entry {
    registry
        .read(cx)
        .entries(Slot::Project, cx)
        .into_iter()
        .find(|entry| entry.key.package.name == "commands" && entry.key.id == id)
        .unwrap()
}

fn requests(fixture: &Fixture) -> Vec<Request> {
    fixture
        .transport
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter(|request| {
            matches!(
                request.command,
                Command::CreateTerminal(_) | Command::OpenToolTerminal { .. }
            )
        })
        .cloned()
        .collect()
}

fn launches(fixture: &Fixture) -> Option<String> {
    std::fs::read_to_string(fixture.directory.path().join("project/starts.txt")).ok()
}

fn pane(
    fixture: &Fixture,
    shell: &Entity<Shell>,
    id: TerminalId,
    visual: &mut VisualTestContext,
) -> Entity<Panel> {
    let target = Target::Terminal(fixture.node.id(), fixture.session.worktree, id);
    wait(visual, |cx| {
        shell
            .read(cx)
            .plugin_panes
            .panels
            .get(&target)
            .and_then(|panel| panel.read(cx).mounted.as_ref())
            .and_then(|mounted| mounted.terminals.first())
            .is_some_and(|view| view.read(cx).info().is_some())
    });
    shell.read_with(visual, |shell, cx| {
        let panel = shell.plugin_panes.panels[&target].clone();
        let state = panel.read(cx);
        assert_eq!(state.binding.client.target(), fixture.node.id());
        assert_eq!(state.binding.worktree, Some(fixture.session.worktree));
        assert_eq!(state.session, None);
        assert_eq!(shell.splits.read(cx).active, Some(target));
        assert_eq!(
            shell
                .splits
                .read(cx)
                .pane(target)
                .unwrap()
                .read(cx)
                .extension,
            Some(Extension {
                package: "commands".into()
            })
        );
        panel
    })
}

#[gpui::test]
fn toolbar_uses_scoped_rust_terminals(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        programs(&fixture);
        let package = install(&fixture);
        let (shell, registry, visual) = mount(&fixture, remote, cx);
        assert!(terminals(&fixture).is_empty());
        visual.update(|_, cx| {
            let cli = entry(&registry, "terminal-cli", cx);
            let choices = cli.choices();
            assert!(
                !choices
                    .iter()
                    .find(|choice| choice.id == "codex")
                    .unwrap()
                    .enabled
            );
            assert!(
                choices
                    .iter()
                    .find(|choice| choice.id == "claude")
                    .unwrap()
                    .enabled
            );
            assert_eq!(cli.key.node, fixture.node.id());
            assert_eq!(cli.key.worktree, Some(fixture.session.worktree));
            assert_eq!(cli.key.session, None);
        });
        click(visual, "plugin-control-commands-terminal-new");
        wait(visual, |_| terminals(&fixture).len() == 1);
        let terminal = terminals(&fixture).remove(0);
        assert_eq!(terminal.tool, None);
        let panel = pane(&fixture, &shell, terminal.id, visual);
        visual.simulate_input("printf 'project-%s\\n' rust-terminal");
        visual.simulate_keystrokes("enter");
        wait(visual, |_| {
            content(&fixture, terminal.id).contains("project-rust-terminal")
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.page = Page::Project;
                cx.notify();
            })
        });
        wait(visual, |cx| {
            entry(&registry, "terminal-cli", cx).state.enabled
        });
        click(visual, "plugin-control-commands-terminal-cli");
        // Kit first selects row zero; the next Down reaches the available Claude entry.
        visual.simulate_keystrokes("down down enter");
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        wait(visual, |_| terminals(&fixture).len() == 2);
        let cli = terminals(&fixture)
            .into_iter()
            .find(|item| item.tool == Some(Tool::Claude))
            .unwrap();
        let cli_panel = pane(&fixture, &shell, cli.id, visual);
        wait(visual, |_| {
            content(&fixture, cli.id).contains("Project CLI ready")
        });
        assert_eq!(launches(&fixture).as_deref(), Some("started\n"));
        let calls = requests(&fixture);
        assert_eq!(calls.len(), 2, "remote={remote}, calls={calls:?}");
        for request in calls {
            assert_eq!(request.target, fixture.node.id());
            let context = request.plugin.unwrap();
            assert_eq!(context.worktree, Some(fixture.session.worktree));
            assert_eq!(context.session, None);
        }
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.page = Page::Project;
                cx.notify();
            })
        });
        fixture.execute(Command::SetPluginEnabled {
            name: "commands".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            registry.read(cx).entries(Slot::Project, cx).is_empty()
        });
        assert!(
            visual
                .debug_bounds("plugin-control-commands-terminal-new")
                .is_none()
        );
        assert!(visual.debug_bounds("live-terminals").is_none());
        assert!(visual.debug_bounds("live-cli").is_none());
        assert_eq!(
            terminals(&fixture)
                .iter()
                .filter(|item| item.status == Status::Running)
                .count(),
            2
        );
        visual.update(|window, _| window.remove_window());
        drop((panel, cli_panel, registry, shell));
        fixture.close();
    }
}

#[gpui::test]
fn loads_menu_for_unselected_project(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        programs(&fixture);
        install(&fixture);
        let path = fixture.directory.path().join("second-project");
        std::fs::create_dir(&path).unwrap();
        git2::Repository::init(&path).unwrap();
        let Output::Project(project) = fixture.execute(Command::RegisterProject {
            name: "Menu fixture".into(),
            path: path.to_str().unwrap().into(),
        }) else {
            panic!("project expected");
        };
        let (shell, current, visual) = mount(&fixture, remote, cx);
        let (registry, worktree) = visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let worktree = shell
                    .live
                    .as_ref()
                    .unwrap()
                    .project_worktree(project.id)
                    .unwrap()
                    .id;
                shell.show_live_project_menu(project.id, point(px(180.), px(180.)), window, cx);
                // Capture the new registry before any asynchronous metadata callback runs.
                let registry = shell.project_plugins(project.id, window, cx).unwrap();
                assert_ne!(registry, current);
                assert!(!registry.read(cx).ready(cx));
                (registry, worktree)
            })
        });
        wait(visual, |cx| {
            let registry = registry.read(cx);
            let entries: Vec<_> = registry
                .entries(Slot::ProjectMenu, cx)
                .into_iter()
                .filter(|entry| entry.key.package.name == "commands")
                .collect();
            registry.ready(cx)
                && entries.len() == 2
                && entries.iter().all(|entry| entry.state.enabled)
        });
        visual.update(|_, cx| {
            let entries: Vec<_> = registry
                .read(cx)
                .entries(Slot::ProjectMenu, cx)
                .into_iter()
                .filter(|entry| entry.key.package.name == "commands")
                .collect();
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.key.id.as_str())
                    .collect::<Vec<_>>(),
                ["terminal-menu-new", "terminal-menu-cli"]
            );
            for entry in &entries {
                assert_eq!(entry.key.node, fixture.node.id());
                assert_eq!(entry.key.worktree, Some(worktree));
                assert_eq!(entry.key.session, None);
            }
            let choices = entries[1].choices();
            assert!(
                choices
                    .iter()
                    .any(|choice| choice.id == "claude" && choice.enabled)
            );
            assert!(
                choices
                    .iter()
                    .any(|choice| choice.id == "codex" && !choice.enabled)
            );
            assert_eq!(choices.last().unwrap().id, "refresh");
            let shell = shell.read(cx);
            let live = shell.live.as_ref().unwrap();
            assert_eq!(live.selected, fixture.node.id());
            assert_eq!(live.project, fixture.binding.project);
            assert_eq!(
                live.selected_worktree().unwrap().id,
                fixture.session.worktree
            );
            assert_eq!(shell.page, Page::Project);
        });
        assert!(requests(&fixture).is_empty());
        assert!(terminals(&fixture).is_empty());
        visual.update(|window, _| window.remove_window());
        drop((registry, current, shell));
        fixture.close();
    }
}

#[gpui::test]
fn restores_after_background_reenable(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        let package = install(&fixture);
        let (shell, registry, visual) = mount(&fixture, remote, cx);
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.controller.id(), cx);
            });
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
        fixture.execute(Command::SetPluginEnabled {
            name: "commands".into(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            let entries = registry.read(cx).entries(Slot::Project, cx);
            entries.len() == 2 && entries.iter().all(|entry| !entry.state.enabled)
        });
        fixture.execute(Command::SetPluginEnabled {
            name: "commands".into(),
            expected_revision: package.summary.revision + 1,
            enabled: true,
        });
        wait(visual, |cx| {
            let entries = registry.read(cx).entries(Slot::Project, cx);
            entries.len() == 2 && entries.iter().all(|entry| entry.state.enabled)
        });
        assert!(terminals(&fixture).is_empty());
        assert!(requests(&fixture).is_empty());
        shell.read_with(visual, |shell, _| {
            assert_eq!(
                shell.live.as_ref().unwrap().selected,
                fixture.controller.id()
            );
            assert_eq!(shell.page, Page::Project);
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(fixture.node.id(), cx);
            });
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
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let project = fixture.binding.project.unwrap();
                shell.live.as_mut().unwrap().project = Some(project);
                assert_eq!(
                    shell.project_plugins(project, window, cx),
                    Some(registry.clone())
                );
                cx.notify();
            });
        });
        wait(visual, |cx| {
            let state = shell.read(cx);
            let live = state.live.as_ref().unwrap();
            state.page == Page::Project
                && live.project == fixture.binding.project
                && registry.read(cx).ready(cx)
                && entry(&registry, "terminal-new", cx).state.enabled
        });
        click(visual, "plugin-control-commands-terminal-new");
        wait(visual, |_| terminals(&fixture).len() == 1);
        let terminal = terminals(&fixture).remove(0);
        let panel = pane(&fixture, &shell, terminal.id, visual);
        let calls = requests(&fixture);
        assert_eq!(calls.len(), 1, "remote={remote}, calls={calls:?}");
        assert_eq!(calls[0].target, fixture.node.id());
        let context = calls[0].plugin.as_ref().unwrap();
        assert_eq!(context.worktree, Some(fixture.session.worktree));
        assert_eq!(context.session, None);
        visual.update(|window, _| window.remove_window());
        drop((panel, registry, shell));
        fixture.close();
    }
}

#[gpui::test]
fn retry_keeps_original_target(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        programs(&fixture);
        install(&fixture);
        let (shell, registry, visual) = mount(&fixture, remote, cx);
        fixture.transport.mode.store(12, Ordering::SeqCst);
        click(visual, "plugin-control-commands-terminal-cli");
        visual.simulate_keystrokes("down down enter");
        wait(visual, |cx| {
            entry(&registry, "terminal-cli", cx)
                .choices()
                .iter()
                .any(|choice| choice.id == "retry" && choice.enabled)
        });
        assert_eq!(requests(&fixture).len(), 1);
        wait(visual, |_| {
            launches(&fixture).as_deref() == Some("started\n")
        });
        visual.update(|_, cx| {
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
        // Kit toast placement uses its animation clock independently of the test clock.
        visual
            .background_executor
            .advance_clock(Duration::from_millis(500));
        std::thread::sleep(Duration::from_millis(450));
        visual.run_until_parked();
        click(visual, "plugin-toast-action-commands-terminal-launch-retry");
        wait(visual, |cx| {
            requests(&fixture).len() == 2
                && !entry(&registry, "terminal-cli", cx)
                    .choices()
                    .iter()
                    .any(|choice| choice.id == "retry")
        });
        let calls = requests(&fixture);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], calls[1]);
        assert_eq!(calls[0].target, fixture.node.id());
        let context = calls[0].plugin.as_ref().unwrap();
        assert_eq!(context.worktree, Some(fixture.session.worktree));
        assert_eq!(context.session, None);
        assert_eq!(launches(&fixture).as_deref(), Some("started\n"));
        let items = terminals(&fixture);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].tool, Some(Tool::Claude));
        shell.read_with(visual, |shell, _| {
            assert_eq!(
                shell.live.as_ref().unwrap().selected,
                fixture.controller.id()
            );
            assert_eq!(shell.page, Page::Project);
        });
        // Kit retains dismissed notifications for its 200 ms exit transition.
        visual
            .background_executor
            .advance_clock(Duration::from_millis(250));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual
                .debug_bounds("plugin-toast-action-commands-terminal-launch-retry")
                .is_none()
        );
        visual.update(|window, _| window.remove_window());
        drop((registry, shell));
        fixture.close();
    }
}
