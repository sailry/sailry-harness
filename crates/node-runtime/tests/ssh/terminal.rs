use super::*;
use sailry_client::terminal::Projection;
use sailry_link::{Subscription, Transport};
use sailry_protocol::terminal::*;
use std::sync::Arc;

fn launch() -> sailry_protocol::ssh::TerminalLaunch {
    sailry_protocol::ssh::TerminalLaunch {
        viewport: Viewport {
            columns: 80,
            rows: 24,
            pixel_width: 640,
            pixel_height: 384,
        },
        appearance: Appearance {
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
        },
    }
}

async fn input(client: &Client, info: &Info, text: &str) {
    execute(
        client,
        Command::InputTerminal {
            terminal: info.id,
            revision: info.revision,
            input: Input::Paste { text: text.into() },
        },
    )
    .await;
    execute(
        client,
        Command::InputTerminal {
            terminal: info.id,
            revision: info.revision,
            input: Input::Key {
                event: KeyEvent {
                    key: Key::Enter,
                    action: Action::Press,
                    modifiers: Modifiers::default(),
                    utf8: None,
                    unshifted_codepoint: None,
                },
            },
        },
    )
    .await;
}

fn content(snapshot: &sailry_protocol::terminal::Snapshot) -> String {
    snapshot
        .screen
        .scrollback
        .iter()
        .chain(&snapshot.screen.rows)
        .flat_map(|row| row.spans.iter().map(|span| span.text.as_str()))
        .collect()
}

async fn until(
    stream: &mut dyn Subscription,
    projection: &mut Projection,
    ready: impl Fn(&sailry_protocol::terminal::Snapshot) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            projection.apply(1, stream.next().await.unwrap()).unwrap();
            if projection.snapshot().is_some_and(&ready) {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "SSH terminal output deadline: {:?}",
            projection
                .snapshot()
                .map(|snapshot| (&snapshot.info.status, content(snapshot)))
        )
    });
}

#[tokio::test]
async fn shares_shell_lifecycle() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("ssh-target");
        std::fs::create_dir(&root).unwrap();
        let server = server::Server::start(root.clone(), 47).await;
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transports: [Arc<dyn Transport>; 2] =
            [node.local(), controller.handle().remote(address)];
        let client = Client::new(transports[usize::from(remote)].clone());
        let other = Client::new(transports[usize::from(!remote)].clone());
        let mut profile = save(
            &client,
            profile(server.port),
            Some(Credential::Password {
                password: Secret::new("isolated-ssh-password".into()),
            }),
        )
        .await;
        let open = |profile: &Profile| Command::OpenSshTerminal {
            profile: profile.id,
            expected_revision: profile.revision,
            launch: launch(),
        };
        assert_eq!(
            execute(&client, open(&profile)).await,
            Output::SshOutcome(Outcome::HostKeyRequired {
                key: server.key.clone(),
                changed: false
            })
        );
        assert_eq!(server.authentication.load(Ordering::SeqCst), 0);
        assert!(server.commands.lock().unwrap().is_empty());
        profile = trust(&client, &profile, &server.key).await;
        let request = client.prepare(open(&profile));
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let output = admitted.completion.await.unwrap().unwrap();
        let Output::SshOutcome(Outcome::Terminal(info)) = &output else {
            panic!("SSH terminal expected: {output:?}")
        };
        assert_eq!(info.ssh, Some(profile.id));
        assert_eq!(info.worktree, None);
        assert_eq!(client.execute(request).await.unwrap(), output);
        assert_eq!(
            server.commands.lock().unwrap().as_slice(),
            ["interactive-shell"]
        );
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        input(&client, info, "printf '%s%s\\n' SSH _READY").await;
        until(&mut *stream, &mut projection, |snapshot| {
            content(snapshot).contains("SSH_READY")
        })
        .await;
        assert_eq!(
            other
                .execute(other.prepare(Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input: Input::Paste {
                        text: "blocked".into()
                    }
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        execute(
            &client,
            Command::ResizeTerminal {
                terminal: info.id,
                revision: info.revision,
                viewport: Viewport {
                    columns: 100,
                    rows: 33,
                    pixel_width: 900,
                    pixel_height: 600,
                },
            },
        )
        .await;
        input(&client, info, "stty size; pwd").await;
        until(&mut *stream, &mut projection, |snapshot| {
            content(snapshot).contains("33 100")
                && content(snapshot).contains(root.to_str().unwrap())
        })
        .await;
        drop(stream);
        controller.handle().disconnect(node.id()).await;
        let mut stream = other.subscribe_terminal(info.id).await.unwrap();
        let Update::TerminalSnapshot(recovered) = stream.next().await.unwrap() else {
            panic!("terminal snapshot expected")
        };
        assert!(content(&recovered).contains("SSH_READY"));
        assert_eq!(recovered.info.owner, info.owner);
        let Output::Terminal(claimed) = execute(
            &other,
            Command::ClaimTerminal {
                terminal: info.id,
                expected_revision: info.revision,
            },
        )
        .await
        else {
            panic!("claimed terminal expected")
        };
        input(&other, &claimed, "exit 7").await;
        let mut projection = Projection::new(node.id(), info.id, 1);
        projection
            .apply(1, Update::TerminalSnapshot(recovered))
            .unwrap();
        until(&mut *stream, &mut projection, |snapshot| {
            snapshot.info.status == Status::Exited { code: 7 }
        })
        .await;
        assert!(projection.snapshot().unwrap().info.owner.is_none());
        execute(&other, Command::CloseSshTerminal { terminal: info.id }).await;
        let Output::SshOutcome(Outcome::Terminal(second)) = execute(&client, open(&profile)).await
        else {
            panic!("SSH terminal expected")
        };
        execute(
            &client,
            Command::CloseSshTerminal {
                terminal: second.id,
            },
        )
        .await;
        assert_eq!(
            client
                .execute(client.prepare(Command::InspectTerminal {
                    terminal: second.id
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        drop(stream);
        node.shutdown().await.unwrap();
        server.close().await;
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_shell_after_profile_removal() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("ssh-target");
        std::fs::create_dir(&root).unwrap();
        let server = server::Server::start(root, 48).await;
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transports: [Arc<dyn Transport>; 2] =
            [node.local(), controller.handle().remote(address)];
        let client = Client::new(transports[usize::from(remote)].clone());
        let other = Client::new(transports[usize::from(!remote)].clone());
        let profile = save(
            &client,
            profile(server.port),
            Some(Credential::Password {
                password: Secret::new("isolated-ssh-password".into()),
            }),
        )
        .await;
        let profile = trust(&client, &profile, &server.key).await;
        let Output::SshOutcome(Outcome::Terminal(info)) = execute(
            &client,
            Command::OpenSshTerminal {
                profile: profile.id,
                expected_revision: profile.revision,
                launch: launch(),
            },
        )
        .await
        else {
            panic!("SSH terminal expected")
        };
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        input(&client, &info, "printf '%s%s\\n' SSH _CAPTURED").await;
        until(&mut *stream, &mut projection, |snapshot| {
            content(snapshot).contains("SSH_CAPTURED")
        })
        .await;
        assert_eq!(
            execute(
                &client,
                Command::RemoveSsh {
                    profile: profile.id,
                    expected_revision: profile.revision,
                },
            )
            .await,
            Output::SshProfiles(Vec::new())
        );
        let Output::Terminals(terminals) = execute(
            &client,
            Command::ListSshTerminals {
                profile: profile.id,
            },
        )
        .await
        else {
            panic!("SSH terminal list expected")
        };
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals[0].id, info.id);
        assert_eq!(terminals[0].status, Status::Running);
        assert_eq!(terminals[0].owner, info.owner);
        assert_eq!(
            execute(
                &other,
                Command::ListSshTerminals {
                    profile: profile.id
                }
            )
            .await,
            Output::Terminals(Vec::new())
        );
        assert_eq!(
            other
                .execute(other.prepare(Command::InputTerminal {
                    terminal: info.id,
                    revision: info.revision,
                    input: Input::Paste {
                        text: "blocked".into()
                    }
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        input(&client, &info, "printf '%s%s\\n' SSH _AFTER_REMOVE").await;
        until(&mut *stream, &mut projection, |snapshot| {
            content(snapshot).contains("SSH_AFTER_REMOVE")
        })
        .await;
        input(&client, &info, "exit 9").await;
        until(&mut *stream, &mut projection, |snapshot| {
            snapshot.info.status == Status::Exited { code: 9 }
        })
        .await;
        let Output::TerminalSnapshot(snapshot) =
            execute(&client, Command::InspectTerminal { terminal: info.id }).await
        else {
            panic!("terminal snapshot expected")
        };
        assert_eq!(snapshot.info.status, Status::Exited { code: 9 });
        assert!(snapshot.info.owner.is_none());
        assert!(content(&snapshot).contains("SSH_CAPTURED"));
        assert!(content(&snapshot).contains("SSH_AFTER_REMOVE"));
        for controller in [&client, &other] {
            assert_eq!(
                execute(
                    controller,
                    Command::ListSshTerminals {
                        profile: profile.id
                    }
                )
                .await,
                Output::Terminals(Vec::new())
            );
        }
        let Output::Terminal(closed) =
            execute(&client, Command::CloseSshTerminal { terminal: info.id }).await
        else {
            panic!("closed terminal expected")
        };
        assert_eq!(closed.status, Status::Closed);
        assert_eq!(
            client
                .execute(client.prepare(Command::InspectTerminal { terminal: info.id }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            server.commands.lock().unwrap().as_slice(),
            ["interactive-shell"]
        );
        drop(stream);
        node.shutdown().await.unwrap();
        server.close().await;
        controller.close().await.unwrap();
    }
}
