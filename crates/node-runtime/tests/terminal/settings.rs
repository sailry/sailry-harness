use super::*;

#[tokio::test]
#[cfg(unix)]
async fn applies_execution_settings() {
    let directory = tempfile::tempdir().unwrap();
    let profile = directory.path().join("node");
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start(&profile).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let transports: [Arc<dyn Transport>; 2] = [node.local(), controller.handle().remote(address)];
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Terminal settings".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let tree = snapshot.worktrees[0].id;
    let mut last = Settings::default();
    for (index, transport) in transports.into_iter().enumerate() {
        let client = Client::new(transport);
        let request = client.prepare(Command::ListShells);
        assert!(!request.command.durable());
        let Output::Shells(shells) = client.execute(request).await.unwrap() else {
            panic!("shells expected")
        };
        assert!(shells.iter().any(|shell| shell == "/bin/sh"));
        assert!(shells.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            shells
                .iter()
                .all(|shell| std::path::Path::new(shell).is_file())
        );
        let Output::TerminalSettings(mut settings) = client
            .execute(client.prepare(Command::ReadTerminalSettings))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        assert_eq!(settings, last);
        settings.shell = "/bin/sh".into();
        let marker = format!("execution-value-{index}");
        settings
            .environment
            .insert("TERMINAL_FIXTURE".into(), marker.clone());
        let request = client.prepare(Command::SaveTerminalSettings(settings.clone()));
        let saved = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), saved);
        let Output::TerminalSettings(updated) = saved else {
            panic!("settings expected")
        };
        assert_eq!(updated.revision, settings.revision + 1);
        assert_eq!(
            client
                .execute(client.prepare(Command::SaveTerminalSettings(settings)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.terminal_settings_revision, updated.revision);
        assert!(!serde_json::to_string(&snapshot).unwrap().contains(&marker));
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(launch(tree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        command(
            &client,
            &info,
            "printf '%s:%s\\n' \"$0\" \"$TERMINAL_FIXTURE\"",
        )
        .await;
        until(&mut *stream, &mut projection, &format!("-sh:{marker}")).await;
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree: tree,
                terminal: info.id,
            }))
            .await
            .unwrap();
        let mut invalid = updated.clone();
        invalid.environment.insert("TERM".into(), "bad".into());
        assert_eq!(
            client
                .execute(client.prepare(Command::SaveTerminalSettings(invalid)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        last = updated;
    }
    let events = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    assert_eq!(
        events
            .query_row(
                "SELECT count(*) FROM events WHERE CAST(body AS TEXT) LIKE '%execution-value-%'",
                [],
                |row| row.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    drop(events);
    node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    assert_eq!(
        client
            .execute(client.prepare(Command::ReadTerminalSettings))
            .await
            .unwrap(),
        Output::TerminalSettings(last)
    );
    node.shutdown().await.unwrap();
    controller.close().await.unwrap();
}

#[tokio::test]
#[cfg(target_os = "macos")]
async fn loads_shell_configuration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let home = directory.path().join("shell-home");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::write(
        home.join(".zprofile"),
        "export SAILRY_LOGIN_FIXTURE=login\n",
    )
    .unwrap();
    std::fs::write(
        home.join(".zshrc"),
        "export SAILRY_INTERACTIVE_FIXTURE=interactive\n",
    )
    .unwrap();
    std::fs::write(
        home.join(".bash_profile"),
        "export SAILRY_LOGIN_FIXTURE=login\n. \"$HOME/.bashrc\"\n",
    )
    .unwrap();
    std::fs::write(
        home.join(".bashrc"),
        "export SAILRY_INTERACTIVE_FIXTURE=interactive\n",
    )
    .unwrap();
    let node = Node::start(directory.path().join("node")).await.unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let client = Client::new(node.local());
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Login fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    for shell in ["/bin/zsh", "/bin/bash"] {
        for transport in [node.local(), controller.handle().remote(address.clone())] {
            let client = Client::new(transport);
            let Output::TerminalSettings(mut settings) = client
                .execute(client.prepare(Command::ReadTerminalSettings))
                .await
                .unwrap()
            else {
                panic!("terminal settings expected")
            };
            settings.shell = shell.into();
            settings
                .environment
                .insert("HOME".into(), home.to_str().unwrap().into());
            client
                .execute(client.prepare(Command::SaveTerminalSettings(settings)))
                .await
                .unwrap();
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let tree = snapshot.worktrees[0].id;
            let Output::Terminal(info) = client
                .execute(client.prepare(Command::CreateTerminal(launch(tree))))
                .await
                .unwrap()
            else {
                panic!("terminal expected")
            };
            let mut stream = client.subscribe_terminal(info.id).await.unwrap();
            let mut projection = Projection::new(node.id(), info.id, 1);
            let inspect = if shell.ends_with("zsh") {
                "printf '%s:%s:%s:%s:%s\\n' \"$SAILRY_LOGIN_FIXTURE\" \"$SAILRY_INTERACTIVE_FIXTURE\" \"$options[login]\" \"$options[interactive]\" \"$PWD\""
            } else {
                "printf '%s:%s:%s:%s:%s\\n' \"$SAILRY_LOGIN_FIXTURE\" \"$SAILRY_INTERACTIVE_FIXTURE\" \"$(shopt -q login_shell && printf on)\" \"$([[ $- == *i* ]] && printf on)\" \"$PWD\""
            };
            command(&client, &info, inspect).await;
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    projection.apply(1, stream.next().await.unwrap()).unwrap();
                    if projection.snapshot().is_some_and(|snapshot| {
                        content(&snapshot.screen).contains("login:interactive:on:on:")
                    }) {
                        break;
                    }
                }
            })
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "login output deadline for {shell}: {}",
                    content(&projection.snapshot().unwrap().screen)
                )
            });
            assert!(
                content(&projection.snapshot().unwrap().screen).contains(root.to_str().unwrap())
            );
            if shell.ends_with("zsh") {
                wait_activity(&mut *stream, &mut projection, Activity::Idle).await;
                assert!(
                    projection
                        .snapshot()
                        .unwrap()
                        .info
                        .directory
                        .as_ref()
                        .unwrap()
                        .ends_with("/project")
                );
                command(&client, &info, "read 'reply?WAITING_FOR_INPUT'").await;
                until(&mut *stream, &mut projection, "WAITING_FOR_INPUT").await;
                assert_eq!(
                    projection.snapshot().unwrap().info.activity.unwrap().state,
                    Activity::Idle
                );
                command(&client, &info, "done").await;
                wait_activity(&mut *stream, &mut projection, Activity::Idle).await;
            } else {
                command(&client, &info, "if [[ -o posix ]]; then printf 'unexpected-mode\\n'; else printf '%s-%s\\n' native bash; fi").await;
                until(&mut *stream, &mut projection, "native-bash").await;
                assert_eq!(projection.snapshot().unwrap().info.activity, None);
            }
            client
                .execute(client.prepare(Command::CloseTerminal {
                    worktree: tree,
                    terminal: info.id,
                }))
                .await
                .unwrap();
        }
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[cfg(target_os = "macos")]
async fn wait_activity(
    stream: &mut dyn Subscription,
    projection: &mut Projection,
    state: Activity,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if projection.snapshot().is_some_and(|snapshot| {
                snapshot
                    .info
                    .activity
                    .is_some_and(|activity| activity.state == state)
            }) {
                break;
            }
            projection.apply(1, stream.next().await.unwrap()).unwrap();
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "shell activity deadline for {state:?}: {:?}; {}",
            projection.snapshot().unwrap().info.activity,
            content(&projection.snapshot().unwrap().screen)
        )
    });
}
