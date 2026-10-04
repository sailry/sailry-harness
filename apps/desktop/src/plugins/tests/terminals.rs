#![cfg(unix)]
use super::*;
use sailry_protocol::{
    Output, TerminalId,
    terminal::{Info, Status},
};

mod panes;
mod project;

fn fixture(remote: bool) -> Fixture {
    let mut fixture = Fixture::new(remote);
    // Production bindings retain controller defaults separately from the execution Node.
    if remote {
        fixture.binding.defaults =
            std::sync::Arc::new(sailry_client::Client::new(fixture.controller.local()));
    }
    fixture
}

fn terminals(fixture: &Fixture) -> Vec<Info> {
    let Output::Terminals(items) = fixture.execute(Command::ListTerminals {
        worktree: fixture.session.worktree,
    }) else {
        panic!()
    };
    items
}

fn content(fixture: &Fixture, id: TerminalId) -> String {
    let Output::TerminalSnapshot(snapshot) =
        fixture.execute(Command::InspectTerminal { terminal: id })
    else {
        panic!()
    };
    snapshot
        .screen
        .scrollback
        .iter()
        .chain(&snapshot.screen.rows)
        .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
        .collect()
}

fn install(fixture: &Fixture) -> sailry_protocol::plugin::Info {
    let root = fixture.directory.path().join("project/commands");
    super::super::fixture::copy_package(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/commands"),
        &root,
    );
    // Kit's JS buttons do not publish layout selectors; anchor their labels for clicks.
    let path = root.join("dev.sailry.platform/desktop/view.js");
    let source = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Button, Tab", "Button as KitButton, Tab");
    std::fs::write(path, format!(
        "import {{Anchor}} from 'sailry/test';\n{source}\nfunction Button(id) {{ const button = new KitButton(id); button.label = text => button.child(Anchor.new(id).child(div().child(text))); return button; }}\n"
    )).unwrap();
    let Output::Plugin(mut package) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "commands".into(),
        name: "commands".into(),
        expected_revision: revision(fixture),
    }) else {
        panic!()
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    if !package.summary.enabled {
        let Output::Plugin(enabled) = fixture.execute(Command::SetPluginEnabled {
            name: package.summary.name.clone(),
            expected_revision: package.summary.revision,
            enabled: true,
        }) else {
            panic!("plugin expected")
        };
        package = enabled;
    }
    package
}

fn revision(fixture: &Fixture) -> u64 {
    let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
        panic!("snapshot expected")
    };
    snapshot
        .plugins
        .iter()
        .find(|package| package.name == "commands")
        .map_or(0, |package| package.revision)
}

fn open<'a>(
    fixture: &Fixture,
    cx: &'a mut TestAppContext,
) -> (Entity<Panel>, &'a mut VisualTestContext) {
    let package = install(fixture);
    let (panel, visual) = mount(fixture, cx);
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        })
    });
    wait(visual, |cx| snapshot(&panel, cx).contains("commands-page"));
    (panel, visual)
}

#[gpui::test]
fn preserves_pty_on_release(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        let (panel, visual) = open(&fixture, cx);
        let revision = revision(&fixture);
        assert!(terminals(&fixture).is_empty());
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(
                    request.command,
                    Command::CreateTerminal(_) | Command::OpenTerminal { .. }
                ))
        );
        click(visual, "terminal-new");
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .and_then(|mounted| mounted.terminals.first())
                .is_some_and(|view| view.read(cx).info().is_some())
        });
        let native = panel.read_with(visual, |panel, _| {
            panel.mounted.as_ref().unwrap().terminals.first().unwrap()
        });
        let first = native.read_with(visual, |view, _| view.info().unwrap().id);
        let previous = native.downgrade();
        visual.update(|window, cx| native.update(cx, |view, cx| view.focus(window, cx)));
        visual.simulate_input("printf 'plugin-%s\\n' rust-terminal");
        visual.simulate_keystrokes("enter");
        wait(visual, |_| {
            content(&fixture, first).contains("plugin-rust-terminal")
        });
        drop(native);
        click(visual, "terminal-new");
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .and_then(|mounted| mounted.terminals.first())
                .is_some_and(|view| view.read(cx).info().is_some_and(|info| info.id != first))
        });
        wait(visual, |_| previous.upgrade().is_none());
        let second = panel.read_with(visual, |panel, cx| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .terminals
                .first()
                .unwrap()
                .read(cx)
                .info()
                .unwrap()
                .id
        });
        assert_eq!(
            terminals(&fixture)
                .iter()
                .filter(|info| info.status == Status::Running)
                .count(),
            2
        );
        fixture.execute(Command::SetPluginEnabled {
            name: "commands".into(),
            expected_revision: revision,
            enabled: false,
        });
        wait(visual, |cx| panel.read(cx).mounted.is_none());
        assert_eq!(
            terminals(&fixture)
                .iter()
                .filter(|info| info.status == Status::Running)
                .count(),
            2
        );
        assert!(
            !fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| matches!(
                    request.command,
                    Command::OpenTerminal { .. } | Command::CloseTerminal { .. }
                ))
        );
        let Output::Plugin(package) = fixture.execute(Command::SetPluginEnabled {
            name: "commands".into(),
            expected_revision: revision + 1,
            enabled: true,
        }) else {
            panic!()
        };
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .and_then(|mounted| mounted.terminals.first())
                .is_some_and(|view| view.read(cx).info().is_some())
        });
        let selected = panel.read_with(visual, |panel, cx| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .terminals
                .first()
                .unwrap()
                .read(cx)
                .info()
                .unwrap()
                .id
        });
        assert!([first, second].contains(&selected));
        let native = panel.read_with(visual, |panel, _| {
            panel.mounted.as_ref().unwrap().terminals.first().unwrap()
        });
        visual.update(|window, cx| native.update(cx, |view, cx| view.focus(window, cx)));
        visual.simulate_input("printf 'preserved-%s\\n' output; exit");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            native
                .read(cx)
                .info()
                .is_some_and(|info| matches!(info.status, Status::Exited { .. }))
                && snapshot(&panel, cx).contains("terminal-content-exited")
        });
        assert!(content(&fixture, selected).contains("preserved-output"));
        assert!(panel.read_with(visual, |panel, _| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .terminals
                .first()
                .is_some_and(|view| view == native)
        }));
        assert!(visual.debug_bounds("terminal-resume").is_none());
        drop(native);
        click(visual, "terminal-close");
        wait(visual, |_| {
            terminals(&fixture)
                .iter()
                .any(|info| info.id == selected && info.status == Status::Closed)
        });
        let requests = fixture.transport.requests.lock().unwrap();
        for request in requests.iter().filter(|request| {
            matches!(
                request.command,
                Command::CreateTerminal(_)
                    | Command::InputTerminal { .. }
                    | Command::CloseTerminal { .. }
            )
        }) {
            let context = request.plugin.as_ref().unwrap();
            assert_eq!(context.worktree, Some(fixture.session.worktree));
            assert_eq!(context.session, Some(fixture.session.id));
            assert_eq!(request.target, fixture.node.id());
        }
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn launches_available_cli(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    init(cx);
    for remote in [false, true] {
        let fixture = fixture(remote);
        let programs = fixture.directory.path().join("programs");
        std::fs::create_dir(&programs).unwrap();
        let program = programs.join("claude");
        let source = concat!(
            "#!/bin/sh\n",
            "printf 'started\\n' >> starts.txt\n",
            "printf 'Fixture CLI ready\\n'\n",
            "while IFS= read -r line; do printf 'Fixture: %s\\n' \"$line\"; done\n",
        );
        std::fs::write(&program, source).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        fixture.execute(Command::SaveTerminalSettings(
            sailry_protocol::terminal::Settings {
                revision: 0,
                shell: String::new(),
                environment: [("PATH".into(), programs.to_str().unwrap().into())].into(),
            },
        ));
        let (panel, visual) = open(&fixture, cx);
        let claude = |cx: &App, available: bool| {
            snapshot(&panel, cx).contains(&format!(
                "Object([(\"id\", Str(\"claude\")), (\"label\", Str(\"Claude\")), (\"enabled\", Bool({available}))])"
            ))
        };
        wait(visual, |cx| claude(cx, true));
        assert!(terminals(&fixture).is_empty());
        click(visual, "terminal-tools");
        // Simulate a confirmed capacity failure; the following attempt launches a real PTY.
        fixture.transport.mode.store(13, Ordering::SeqCst);
        // Kit keyboard navigation skips the unavailable Codex item before Claude.
        visual.simulate_keystrokes("down enter");
        toast(visual, "Terminal action failed");
        wait(visual, |cx| claude(cx, true));
        assert!(panel.read_with(visual, |_, cx| {
            !snapshot(&panel, cx).contains("terminal-error-failed")
        }));
        assert!(terminals(&fixture).is_empty());
        let launches = fixture.directory.path().join("project/starts.txt");
        assert!(!launches.exists());
        let failed = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| matches!(request.command, Command::OpenToolTerminal { .. }))
            .unwrap()
            .clone();
        let sailry_protocol::RequestOutcome::Completed(result) = fixture
            .runtime
            .block_on(fixture.binding.client.outcome(&failed))
            .unwrap()
        else {
            panic!("completed launch expected")
        };
        assert_eq!(result.unwrap_err().code, sailry_protocol::ErrorCode::Busy);
        let refreshes = || {
            fixture
                .transport
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| matches!(request.command, Command::ListTerminalTools { .. }))
                .count()
        };
        let before = refreshes();
        let (package, root) = panel.read_with(visual, |panel, _| {
            (
                panel.selected.clone().unwrap(),
                panel.mounted.as_ref().unwrap().root.entity_id(),
            )
        });
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.enter(package, window, cx));
        });
        wait(visual, |cx| refreshes() > before && claude(cx, true));
        assert_eq!(
            panel.read_with(visual, |panel, _| {
                panel.mounted.as_ref().unwrap().root.entity_id()
            }),
            root,
        );
        click(visual, "terminal-tools");
        visual.simulate_keystrokes("down enter");
        wait(visual, |_| terminals(&fixture).len() == 1);
        let terminal = terminals(&fixture).remove(0);
        assert_eq!(terminal.tool, Some(sailry_protocol::terminal::Tool::Claude));
        wait(visual, |_| {
            content(&fixture, terminal.id).contains("Fixture CLI ready")
        });
        assert_eq!(std::fs::read_to_string(launches).unwrap(), "started\n");
        let requests = fixture.transport.requests.lock().unwrap();
        let starts: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::OpenToolTerminal { .. }))
            .collect();
        assert_eq!(starts.len(), 2);
        assert_eq!(starts[0].id, failed.id);
        assert_ne!(starts[0].id, starts[1].id);
        for request in starts {
            assert_eq!(request.target, fixture.node.id());
            let context = request.plugin.as_ref().unwrap();
            assert_eq!(context.worktree, Some(fixture.session.worktree));
            assert_eq!(context.session, Some(fixture.session.id));
        }
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn preserves_shell_title_rules(cx: &mut TestAppContext) {
    init(cx);
    let fixture = fixture(false);
    fixture.package();
    let root = fixture.directory.path().join("project/package");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("plugin.json")).unwrap()).unwrap();
    manifest["extensions"]["dev.sailry.platform"]["desktop"]["resources"]
        .as_array_mut()
        .unwrap()
        .push("dev.sailry.platform/desktop/labels.js".into());
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/desktop/labels.js"),
        include_str!("../../../../../plugins/commands/dev.sailry.platform/desktop/labels.js"),
    )
    .unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/desktop/main.js"),
        include_str!("terminals/labels.js"),
    )
    .unwrap();
    let package = fixture.install(0);
    let (panel, visual) = mount(&fixture, cx);
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        })
    });
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("title-rules-passed")
    });
    visual.update(|window, _| window.remove_window());
    drop(panel);
    fixture.close();
}

#[gpui::test]
fn rejects_unscoped_mounts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for foreign in [false, true] {
            let fixture = fixture(remote);
            let worktree = if foreign {
                let Output::Session(session) = fixture.execute(Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(fixture.session.config.clone()),
                }) else {
                    panic!()
                };
                assert_ne!(session.worktree, fixture.session.worktree);
                session.worktree
            } else {
                fixture.session.worktree
            };
            let appearance = cx.update(|cx| crate::theme::terminal(cx));
            let Output::Terminal(terminal) =
                fixture.execute(Command::CreateTerminal(sailry_protocol::terminal::Launch {
                    worktree,
                    appearance,
                    viewport: sailry_protocol::terminal::Viewport {
                        columns: 80,
                        rows: 24,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                }))
            else {
                panic!()
            };
            let root = fixture.directory.path().join("project/package");
            std::fs::create_dir_all(root.join("dev.sailry.platform/desktop")).unwrap();
            std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
            let mut manifest: serde_json::Value =
                serde_json::from_str(include_str!("../../../../../plugins/commands/plugin.json"))
                    .unwrap();
            manifest["name"] = "project-summary".into();
            let extension = &mut manifest["extensions"]["dev.sailry.platform"];
            extension["desktop"]["resources"] =
                serde_json::json!(["dev.sailry.platform/desktop/main.js"]);
            extension["desktop"]
                .as_object_mut()
                .unwrap()
                .remove("ui_entry");
            extension["desktop"]
                .as_object_mut()
                .unwrap()
                .remove("renderers");
            extension.as_object_mut().unwrap().remove("ui");
            if !foreign {
                extension["actions"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|action| action != "terminals.control");
            }
            std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
            std::fs::write(
                root.join("dev.sailry.platform/host/main.js"),
                include_str!("../../../../../plugins/commands/dev.sailry.platform/host/main.js"),
            )
            .unwrap();
            std::fs::write(
                root.join("dev.sailry.platform/host/locales.js"),
                include_str!("../../../../../plugins/commands/dev.sailry.platform/host/locales.js"),
            )
            .unwrap();
            std::fs::write(
                root.join("dev.sailry.platform/desktop/main.js"),
                format!(
                    "import {{View}} from 'gpui-kit'; import {{Terminal}} from 'sailry';
                    export default class Probe extends View {{
                      render() {{ return Terminal.new('probe', {{terminal:'{}'}}); }}
                    }}",
                    terminal.id
                ),
            )
            .unwrap();
            let package = fixture.install(0);
            assert!(package.issues.is_empty());
            let (panel, visual) = mount(&fixture, cx);
            visual.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.open(package.summary.reference(), window, cx)
                })
            });
            wait(visual, |cx| snapshot(&panel, cx).contains("probe"));
            assert!(visual.debug_bounds("plugin-terminal-unavailable").is_some());
            assert!(panel.read_with(visual, |panel, _| {
                panel.mounted.as_ref().unwrap().terminals.first().is_none()
            }));
            assert!(
                !fixture
                    .transport
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|request| matches!(
                        request.command,
                        Command::OpenTerminal { .. } | Command::ClaimTerminal { .. }
                    ))
            );
            visual.update(|window, _| window.remove_window());
            drop(panel);
            fixture.close();
        }
    }
}
