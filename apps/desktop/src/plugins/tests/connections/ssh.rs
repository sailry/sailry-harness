use super::*;

fn paths_prompt(visual: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !visual.did_prompt_for_paths() {
        visual.run_until_parked();
        assert!(
            Instant::now() < deadline,
            "private key prompt was not opened"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn edits_profiles_on_the_captured_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture, "ssh");
        let (shell, panel, visual) = mount(&fixture, remote, "ssh", cx);
        landing_width(visual, "ssh");
        tap(visual, "ssh-new");
        shown(&panel, visual, "ssh-editor");
        for (id, value) in [
            ("field-4", "Captured SSH"),
            ("field-5", "127.0.0.1"),
            ("field-6", "2222"),
            ("field-7", "fixture"),
            ("secret-1", "isolated-password"),
        ] {
            input(visual, id, value);
        }
        tap(visual, "ssh-save");
        wait(visual, |_| state(&fixture).ssh.len() == 1);
        let profile = state(&fixture).ssh[0].clone();
        assert_eq!(profile.name, "Captured SSH");
        assert_eq!(profile.port, 2222);
        shown(&panel, visual, &format!("ssh-open-{}", profile.id));
        landing_width(visual, "ssh");
        connection_action(&panel, visual, "ssh", "edit");
        shown(&panel, visual, "ssh-editor");
        input(visual, "field-8", "Renamed SSH");
        tap(visual, "ssh-save");
        wait(visual, |_| state(&fixture).ssh[0].revision == 2);
        assert_eq!(state(&fixture).ssh[0].name, "Renamed SSH");
        let requests = fixture.transport.requests.lock().unwrap();
        let saves: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::SaveSsh { .. }))
            .collect();
        assert_eq!(saves.len(), 2);
        assert!(saves.iter().all(|request| {
            request.plugin.as_ref().is_some_and(|context| {
                context.package == package.summary.reference() && context.worktree.is_none()
            })
        }));
        assert!(matches!(
            &saves[1].command,
            Command::SaveSsh {
                credential: None,
                ..
            }
        ));
        drop(requests);
        connection_action(&panel, visual, "ssh", "remove");
        shown(&panel, visual, "ssh-confirm");
        tap(visual, "ssh-confirm-submit");
        wait(visual, |_| state(&fixture).ssh.is_empty());
        let client = sailry_client::Client::new(fixture.controller.local());
        let Output::Snapshot(other) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::Snapshot)))
            .unwrap()
        else {
            panic!("snapshot expected");
        };
        assert!(other.ssh.is_empty());
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}

use crate::ssh_fixture as server;

#[gpui::test]
fn opens_core_terminals_and_changes_remote_files(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture, "ssh");
        let root = fixture.directory.path().join("peer");
        std::fs::create_dir(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let peer = fixture
            .runtime
            .block_on(server::Server::start(root.clone(), 71));
        let Output::SshProfile(profile) = fixture.execute(Command::SaveSsh {
            profile: sailry_protocol::ssh::Profile {
                id: sailry_protocol::SshId::new(),
                revision: 0,
                name: "Isolated SSH".into(),
                host: "127.0.0.1".into(),
                port: peer.port,
                username: "fixture".into(),
                authentication: sailry_protocol::ssh::Authentication::Password,
                host_key: None,
                sharing: None,
            },
            expected_revision: 0,
            credential: Some(sailry_protocol::ssh::Credential::Password {
                password: sailry_protocol::Secret::new("isolated-ssh-password".into()),
            }),
        }) else {
            panic!("profile expected");
        };
        let (shell, panel, visual) = mount(&fixture, remote, "ssh", cx);
        shown(&panel, visual, &format!("ssh-open-{}", profile.id));
        tap(visual, &format!("ssh-open-{}", profile.id));
        shown(&panel, visual, "ssh-host-key");
        assert_eq!(peer.authentication.load(Ordering::SeqCst), 0);
        assert!(
            state(&fixture)
                .terminals
                .iter()
                .all(|terminal| terminal.ssh != Some(profile.id))
        );
        tap(visual, "ssh-trust");
        shown(&panel, visual, "ssh-page");
        shown(&panel, visual, "ssh-files-panel");
        no_session_navigation(visual);
        assert!(visual.debug_bounds("shell-navigation").is_some());
        assert!(visual.debug_bounds("header-sidebar-toggle").is_some());
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .unwrap()
                .terminals
                .first()
                .is_some()
        });
        let terminals: Vec<_> = state(&fixture)
            .terminals
            .into_iter()
            .filter(|terminal| terminal.ssh == Some(profile.id))
            .collect();
        assert_eq!(terminals.len(), 1);
        let terminal = terminals[0].id;
        assert_eq!(
            terminals[0].owner,
            Some(if remote {
                fixture.controller.id()
            } else {
                fixture.node.id()
            })
        );
        assert!(panel.read_with(visual, |panel, cx| {
            panel
                .mounted
                .as_ref()
                .unwrap()
                .conversations
                .first()
                .is_some_and(|chat| chat.read(cx).session().is_none())
        }));
        wait(visual, |cx| {
            snapshot(&panel, cx).lines().any(|line| {
                line.contains("module_component sailry/ui.NativeContextMenu \"ssh-files-root\"")
                    && line.contains("(\"label\", Str(\"New file\")), (\"enabled\", Bool(true))")
            })
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let empty = visual.debug_bounds("empty-ssh-files-empty").unwrap();
        let viewport = visual.debug_bounds("ssh-files-root").unwrap();
        assert_eq!(empty, viewport);
        assert!(visual.debug_bounds("empty-card-ssh-files-empty").is_none());
        let position = empty.center();
        visual.simulate_mouse_down(position, MouseButton::Right, Modifiers::default());
        visual.run_until_parked();
        // Select the actual captured descriptor through its native Dispatch action.
        // TestWindow cannot open AppKit's OS popup; this does not claim OS acceptance.
        visual.update(|window, cx| {
            crate::plugins::native_context::choose("ssh-files-root", "New file", window, cx)
        });
        visual.run_until_parked();
        shown(&panel, visual, "ssh-file-editor");
        input(visual, "field-5", "created.txt");
        tap(visual, "ssh-file-submit");
        wait(visual, |_| root.join("created.txt").exists());
        shown(&panel, visual, "created.txt");
        let create = fixture
            .transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| {
                matches!(
                    &request.command,
                    Command::ModifySshFile {
                        action: sailry_protocol::ssh::FileAction::Create { directory: false },
                        ..
                    }
                )
            })
            .cloned()
            .unwrap();
        assert!(
            create
                .plugin
                .as_ref()
                .is_some_and(|context| context.package == package.summary.reference()
                    && context.worktree.is_none())
        );
        // Native OS drops expose only an opaque source handle to the package.
        let sources = tempfile::tempdir().unwrap();
        let dropped = sources.path().join("Dropped 中文.txt");
        let folder = sources.path().join("folder");
        std::fs::create_dir_all(folder.join("nested")).unwrap();
        std::fs::write(folder.join("nested/item.txt"), b"nested upload").unwrap();
        std::fs::write(&dropped, b"opaque upload").unwrap();
        let position = visual.debug_bounds("ssh-files").unwrap().center();
        visual.simulate_event(FileDropEvent::Entered {
            position,
            paths: ExternalPaths(vec![dropped.clone(), folder].into()),
        });
        visual.update(|window, cx| window.draw(cx).clear(cx));
        visual.simulate_event(FileDropEvent::Submit { position });
        wait(visual, |_| {
            std::fs::read(root.join("folder/nested/item.txt"))
                .is_ok_and(|bytes| bytes == b"nested upload")
                && std::fs::read(root.join("Dropped 中文.txt"))
                    .is_ok_and(|bytes| bytes == b"opaque upload")
        });
        assert_eq!(
            std::fs::read(root.join("folder/nested/item.txt")).unwrap(),
            b"nested upload"
        );
        assert_eq!(
            std::fs::read(root.join("Dropped 中文.txt")).unwrap(),
            b"opaque upload"
        );
        shown(&panel, visual, "Dropped 中文.txt");
        let ui = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(
            !ui.contains(sources.path().to_str().unwrap()),
            "local paths must stay in the native source capability"
        );
        let destination = sources.path().join("downloaded.txt");
        let selector: &'static str = Box::leak(
            format!("resource-file-{}/Dropped 中文.txt", root.display()).into_boxed_str(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        while visual.debug_bounds(selector).is_none() {
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(
                Instant::now() < deadline,
                "uploaded remote row was not rendered"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        tap(visual, selector);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !visual.did_prompt_for_new_path() {
            visual.run_until_parked();
            assert!(
                Instant::now() < deadline,
                "download destination prompt was not opened"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        visual.simulate_new_path_selection(|_| Some(destination.clone()));
        wait(visual, |_| {
            std::fs::read(&destination).is_ok_and(|bytes| bytes == b"opaque upload")
        });
        assert_eq!(std::fs::read(destination).unwrap(), b"opaque upload");
        connection_action(&panel, visual, "ssh", "open");
        shown(&panel, visual, "ssh-page");
        assert_eq!(
            state(&fixture)
                .terminals
                .iter()
                .filter(|entry| entry.ssh == Some(profile.id))
                .count(),
            1,
            "opening an existing connection reuses its Rust terminal"
        );
        let saved = state(&fixture)
            .ssh
            .into_iter()
            .find(|entry| entry.id == profile.id)
            .unwrap();
        fixture.execute(Command::RemoveSsh {
            profile: saved.id,
            expected_revision: saved.revision,
        });
        wait(visual, |cx| {
            let ui = snapshot(&panel, cx);
            ui.contains("ssh-page") && !ui.contains("ssh-files-panel")
        });
        let close = format!("ssh-tab-close-{terminal}");
        shown(&panel, visual, &close);
        let Output::TerminalSnapshot(retained) =
            fixture.execute(Command::InspectTerminal { terminal })
        else {
            panic!("terminal snapshot expected");
        };
        assert_eq!(
            retained.info.status,
            sailry_protocol::terminal::Status::Running
        );
        fixture.execute(Command::InputTerminal {
            terminal,
            revision: retained.info.revision,
            input: sailry_protocol::terminal::Input::Paste {
                text: "printf '%s%s\\n' RETAINED _OUTPUT; exit 7".into(),
            },
        });
        fixture.execute(Command::InputTerminal {
            terminal,
            revision: retained.info.revision,
            input: sailry_protocol::terminal::Input::Key {
                event: sailry_protocol::terminal::KeyEvent {
                    key: sailry_protocol::terminal::Key::Enter,
                    action: sailry_protocol::terminal::Action::Press,
                    modifiers: Default::default(),
                    utf8: None,
                    unshifted_codepoint: None,
                },
            },
        });
        wait(visual, |cx| {
            panel
                .read(cx)
                .mounted
                .as_ref()
                .unwrap()
                .terminals
                .first()
                .is_some_and(|view| {
                    view.read(cx).info().is_some_and(|info| {
                        info.id == terminal
                            && info.owner.is_none()
                            && info.status == sailry_protocol::terminal::Status::Exited { code: 7 }
                    })
                })
        });
        let Output::TerminalSnapshot(exited) =
            fixture.execute(Command::InspectTerminal { terminal })
        else {
            panic!("terminal snapshot expected");
        };
        let output: String = exited
            .screen
            .scrollback
            .iter()
            .chain(&exited.screen.rows)
            .flat_map(|row| row.spans.iter().map(|span| span.text.as_str()))
            .collect();
        assert!(output.contains("RETAINED_OUTPUT"));
        shown(&panel, visual, &close);
        tap(visual, &close);
        wait(visual, |_| {
            state(&fixture).terminals.iter().any(|entry| {
                entry.id == terminal && entry.status == sailry_protocol::terminal::Status::Closed
            })
        });
        assert!(fixture.transport.requests.lock().unwrap().iter().any(|request| {
            matches!(request.command, Command::CloseSshTerminal { terminal: id } if id == terminal)
                && request.plugin.as_ref().is_some_and(|context| {
                    context.package == package.summary.reference() && context.worktree.is_none()
                })
        }));
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.runtime.block_on(peer.close());
        fixture.close();
    }
}

#[gpui::test]
fn closing_tabs_focuses_the_next_terminal(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = install(&fixture, "ssh");
        let root = fixture.directory.path().join("peer");
        std::fs::create_dir(&root).unwrap();
        let peer = fixture.runtime.block_on(server::Server::start(root, 71));
        let Output::SshProfile(profile) = fixture.execute(Command::SaveSsh {
            profile: sailry_protocol::ssh::Profile {
                id: sailry_protocol::SshId::new(),
                revision: 0,
                name: "Isolated SSH".into(),
                host: "127.0.0.1".into(),
                port: peer.port,
                username: "fixture".into(),
                authentication: sailry_protocol::ssh::Authentication::Password,
                host_key: None,
                sharing: None,
            },
            expected_revision: 0,
            credential: Some(sailry_protocol::ssh::Credential::Password {
                password: sailry_protocol::Secret::new("isolated-ssh-password".into()),
            }),
        }) else {
            panic!("profile expected");
        };
        let (shell, panel, visual) = mount(&fixture, remote, "ssh", cx);
        shown(&panel, visual, &format!("ssh-open-{}", profile.id));
        tap(visual, &format!("ssh-open-{}", profile.id));
        shown(&panel, visual, "ssh-host-key");
        tap(visual, "ssh-trust");
        shown(&panel, visual, "ssh-page");
        wait(visual, |_| {
            state(&fixture)
                .terminals
                .iter()
                .any(|info| info.ssh == Some(profile.id))
        });
        let first = state(&fixture)
            .terminals
            .into_iter()
            .find(|info| info.ssh == Some(profile.id))
            .unwrap()
            .id;
        let trusted = state(&fixture)
            .ssh
            .into_iter()
            .find(|item| item.id == profile.id)
            .unwrap();
        let mut opened = vec![first];
        for _ in 0..2 {
            let Output::SshOutcome(sailry_protocol::ssh::Outcome::Terminal(info)) = fixture
                .execute(Command::OpenSshTerminal {
                    profile: trusted.id,
                    expected_revision: trusted.revision,
                    launch: sailry_protocol::ssh::TerminalLaunch {
                        viewport: sailry_protocol::terminal::Viewport {
                            columns: 80,
                            rows: 24,
                            pixel_width: 640,
                            pixel_height: 384,
                        },
                        appearance: visual.update(|_, cx| crate::theme::terminal(cx)),
                    },
                })
            else {
                panic!("terminal expected");
            };
            opened.push(info.id);
        }
        let Output::Terminals(listed) = fixture.execute(Command::ListSshTerminals {
            profile: trusted.id,
        }) else {
            panic!("terminals expected");
        };
        assert_eq!(listed.len(), 3);
        assert!(listed.iter().all(|info| opened.contains(&info.id)));
        opened = listed.into_iter().map(|info| info.id).collect();
        for id in &opened {
            shown(&panel, visual, &format!("ssh-tab-{id}"));
        }
        tap(visual, &format!("ssh-tab-{}", opened[1]));
        terminal_focused(&panel, visual, opened[1]);
        // A middle close chooses the right neighbor, then the left edge. No
        // pointer interaction separates the three scoped durable closes.
        for (closed, next) in [
            (opened[1], Some(opened[2])),
            (opened[2], Some(opened[0])),
            (opened[0], None),
        ] {
            visual.simulate_keystrokes("secondary-w");
            wait(visual, |_| {
                state(&fixture).terminals.iter().any(|info| {
                    info.id == closed && info.status == sailry_protocol::terminal::Status::Closed
                })
            });
            if let Some(next) = next {
                terminal_focused(&panel, visual, next);
            } else {
                shown(&panel, visual, "ssh-landing");
            }
        }
        let requests = fixture.transport.requests.lock().unwrap();
        let closes: Vec<_> = requests
            .iter()
            .filter(|request| matches!(request.command, Command::CloseSshTerminal { .. }))
            .collect();
        assert_eq!(closes.len(), 3);
        assert!(closes.iter().all(|request| {
            request.plugin.as_ref().is_some_and(|context| {
                context.package == package.summary.reference() && context.worktree.is_none()
            })
        }));
        drop(requests);
        assert_eq!(
            panel.read_with(visual, |panel, _| panel.binding.client.target()),
            fixture.node.id()
        );
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.runtime.block_on(peer.close());
        fixture.close();
    }
}

#[track_caller]
fn terminal_focused(
    panel: &Entity<Panel>,
    visual: &mut VisualTestContext,
    id: sailry_protocol::TerminalId,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        visual.run_until_parked();
        if visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            panel
                .read(cx)
                .mounted
                .as_ref()
                .unwrap()
                .terminals
                .focused(id, window, cx)
        }) {
            return;
        }
        if Instant::now() >= deadline {
            let (focus, dialog, retained, connected, loading) = visual.update(|window, cx| {
                let dialog = window.has_active_dialog(cx);
                let panel = panel.read(cx);
                (
                    window.focused(cx),
                    dialog,
                    panel.mounted.as_ref().unwrap().terminals.focus_state(cx),
                    panel.connected,
                    panel.loading,
                )
            });
            panic!(
                "terminal focus deadline: target={id}, current={focus:?}, dialog={dialog}, retained={retained:?}, connected={connected}, loading={loading}, terminal={:?}, unavailable={:?}",
                visual.debug_bounds("terminal-component"),
                visual.debug_bounds("plugin-terminal-unavailable"),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn imports_private_keys_without_exposing_controller_paths(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        install(&fixture, "ssh");
        let path = fixture.directory.path().join("selected_key");
        let key = server::key(43)
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .unwrap();
        std::fs::write(&path, key.as_bytes()).unwrap();
        let (shell, panel, visual) = mount(&fixture, remote, "ssh", cx);
        tap(visual, "ssh-connections-menu");
        shown(&panel, visual, "connection-add");
        tap(visual, "connection-add");
        shown(&panel, visual, "ssh-editor");
        for (id, value) in [
            ("field-4", "Imported key"),
            ("field-5", "127.0.0.1"),
            ("field-6", "2222"),
            ("field-7", "fixture"),
        ] {
            input(visual, id, value);
        }
        tap(visual, "ssh-authentication-1-item-key_path");
        shown(&panel, visual, "ssh-key-choose");
        tap(visual, "ssh-key-choose");
        paths_prompt(visual);
        visual.simulate_path_prompt_response(|options| {
            assert!(options.files && !options.directories && !options.multiple);
            Some(vec![path.clone()])
        });
        shown(&panel, visual, "selected_key");
        let ui = visual.update(|_, cx| snapshot(&panel, cx));
        assert!(!ui.contains(path.to_str().unwrap()));
        assert!(!ui.contains("BEGIN OPENSSH PRIVATE KEY"));
        tap(visual, "ssh-key-choose");
        paths_prompt(visual);
        visual.simulate_path_prompt_response(|_| None);
        shown(&panel, visual, "selected_key");
        tap(visual, "ssh-save");
        wait(visual, |_| state(&fixture).ssh.len() == 1);
        assert_eq!(
            state(&fixture).ssh[0].authentication,
            sailry_protocol::ssh::Authentication::PrivateKey
        );
        let requests = fixture.transport.requests.lock().unwrap();
        let saved = requests
            .iter()
            .find_map(|request| match &request.command {
                Command::SaveSsh {
                    credential: Some(sailry_protocol::ssh::Credential::PrivateKey { key, .. }),
                    ..
                } => Some(key.expose()),
                _ => None,
            })
            .unwrap();
        assert_eq!(saved, key.as_str());
        drop(requests);
        std::fs::remove_file(path).unwrap();
        visual.update(|window, _| window.remove_window());
        drop(panel);
        drop(shell);
        fixture.close();
    }
}
