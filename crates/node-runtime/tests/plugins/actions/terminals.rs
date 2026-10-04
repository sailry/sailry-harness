#![cfg(unix)]
use super::*;
use sailry_protocol::terminal::{self, Appearance, Input, Launch, Status, Viewport};

fn viewport() -> Viewport {
    Viewport {
        columns: 80,
        rows: 24,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn appearance() -> Appearance {
    use terminal::{ColorScheme, Rgb};
    Appearance {
        foreground: Rgb {
            red: 240,
            green: 240,
            blue: 240,
        },
        background: Rgb {
            red: 20,
            green: 20,
            blue: 20,
        },
        palette: [Rgb {
            red: 128,
            green: 128,
            blue: 128,
        }; 16],
        color_scheme: ColorScheme::Dark,
    }
}

#[tokio::test]
async fn scopes_control_and_replays() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let context = install_actions(
            &client,
            &directory.path().join("source/package"),
            worktree,
            0,
            &[Action::ReadTerminals, Action::ControlTerminals],
        )
        .await;
        let launch = client
            .prepare(Command::CreateTerminal(Launch {
                worktree,
                viewport: viewport(),
                appearance: appearance(),
            }))
            .with_plugin(context.clone());
        let Output::Terminal(terminal) = client.execute(launch.clone()).await.unwrap() else {
            panic!()
        };
        assert_eq!(
            client.execute(launch).await.unwrap(),
            Output::Terminal(terminal.clone())
        );
        let Output::Terminals(terminals) = client
            .execute(
                client
                    .prepare(Command::ListTerminals { worktree })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals[0].id, terminal.id);
        let inspect = client
            .prepare(Command::InspectTerminal {
                terminal: terminal.id,
            })
            .with_plugin(context.clone());
        assert!(matches!(
            client.execute(inspect.clone()).await.unwrap(),
            Output::TerminalSnapshot(_)
        ));
        let Output::Terminal(owned) = client
            .execute(
                client
                    .prepare(Command::ClaimTerminal {
                        terminal: terminal.id,
                        expected_revision: terminal.revision,
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap()
        else {
            panic!()
        };
        client
            .execute(
                client
                    .prepare(Command::InputTerminal {
                        terminal: terminal.id,
                        revision: owned.revision,
                        input: Input::Paste {
                            text: "printf 'plugin-%s\\n' pty-check".into(),
                        },
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap();
        client
            .execute(
                client
                    .prepare(Command::InputTerminal {
                        terminal: terminal.id,
                        revision: owned.revision,
                        input: Input::Key {
                            event: terminal::KeyEvent {
                                key: terminal::Key::Enter,
                                action: terminal::Action::Press,
                                modifiers: terminal::Modifiers::default(),
                                utf8: None,
                                unshifted_codepoint: None,
                            },
                        },
                    })
                    .with_plugin(context.clone()),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let Output::TerminalSnapshot(snapshot) =
                    client.execute(inspect.clone()).await.unwrap()
                else {
                    panic!()
                };
                let text = snapshot
                    .screen
                    .scrollback
                    .iter()
                    .chain(&snapshot.screen.rows)
                    .flat_map(|line| line.spans.iter().map(|span| span.text.as_str()))
                    .collect::<String>();
                if text.contains("plugin-pty-check") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        for command in [
            Command::ListTerminals {
                worktree: WorktreeId::new(),
            },
            Command::CloseTerminal {
                worktree: WorktreeId::new(),
                terminal: terminal.id,
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let mut outside = context.clone();
        outside.worktree = None;
        for command in [
            Command::InspectTerminal {
                terminal: terminal.id,
            },
            Command::ClaimTerminal {
                terminal: terminal.id,
                expected_revision: owned.revision,
            },
            Command::OpenTerminal {
                terminal: terminal.id,
                viewport: viewport(),
                appearance: appearance(),
            },
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command).with_plugin(outside.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let closed = client
            .prepare(Command::CloseTerminal {
                worktree,
                terminal: terminal.id,
            })
            .with_plugin(context.clone());
        let result = client.execute(closed.clone()).await.unwrap();
        assert!(matches!(&result,Output::Terminal(info) if info.status == Status::Closed));
        assert_eq!(client.execute(closed).await.unwrap(), result);
        execute(
            &client,
            Command::SetPluginEnabled {
                name: "example".into(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            client.execute(inspect).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
