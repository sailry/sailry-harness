use super::*;
#[path = "../support/environment.rs"]
mod fixture;

impl fixture::Login {
    fn captures(&self) -> usize {
        std::fs::read(self.home.join("captures"))
            .unwrap_or_default()
            .len()
    }
}

async fn open(client: &Client, node: NodeId, worktree: WorktreeId, needle: &str) {
    let Output::Terminal(info) = client
        .execute(client.prepare(Command::OpenToolTerminal {
            tool: Tool::Codex,
            launch: Launch {
                access: sailry_protocol::process::Access::Unrestricted,
                ..launch(worktree)
            },
        }))
        .await
        .unwrap()
    else {
        panic!("tool terminal expected")
    };
    let mut stream = client.subscribe_terminal(info.id).await.unwrap();
    let mut projection = Projection::new(node, info.id, 1);
    until(&mut *stream, &mut projection, needle).await;
    command(client, &info, "fixture-input").await;
    until(&mut *stream, &mut projection, "fixture-input").await;
    client
        .execute(client.prepare(Command::CloseTerminal {
            worktree,
            terminal: info.id,
        }))
        .await
        .unwrap();
}

#[tokio::test]
async fn shares_execution_login_and_setting_overrides() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let login = fixture::Login::new(directory.path());
        let other = directory.path().join("controller-login");
        std::fs::create_dir(&other).unwrap();
        let controller_login = fixture::Login::new(&other);
        controller_login.select(true);
        let profile = directory.path().join("node");
        let node = Node::start_with_environment(&profile, login.source())
            .await
            .unwrap();
        let controller = Node::start_with_environment(
            directory.path().join("controller"),
            controller_login.source(),
        )
        .await
        .unwrap();
        let address = controller
            .link()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.link().remote(address)
        } else {
            node.local()
        });
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Login fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(project.id))
            .unwrap()
            .id;
        let Output::Shells(shells) = client
            .execute(client.prepare(Command::ListShells))
            .await
            .unwrap()
        else {
            panic!("shells expected")
        };
        assert!(shells.iter().any(|shell| shell == "/bin/sh"));
        let Output::TerminalTools(tools) = client
            .execute(client.prepare(Command::ListTerminalTools { worktree }))
            .await
            .unwrap()
        else {
            panic!("tools expected")
        };
        assert!(
            tools
                .iter()
                .any(|tool| tool.tool == Tool::Codex && tool.available)
        );
        assert_eq!(login.captures(), 1);
        assert_eq!(controller_login.captures(), 1);
        open(&client, node.id(), worktree, "tool-ready:first:unset").await;
        login.select(true);
        open(&client, node.id(), worktree, "tool-ready:first:unset").await;
        assert_eq!(login.captures(), 1);
        let Output::TerminalSettings(settings) = client
            .execute(client.prepare(Command::SaveTerminalSettings(Settings {
                revision: 0,
                shell: "/bin/sh".into(),
                environment: Default::default(),
            })))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(Launch {
                access: sailry_protocol::process::Access::Unrestricted,
                ..launch(worktree)
            })))
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
            "printf 'shell:%s:' \"$0\"; environment-fixture; printf '\\n'",
        )
        .await;
        until(&mut *stream, &mut projection, "shell:-sh:first:unset:unset").await;
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            }))
            .await
            .unwrap();
        let Output::TerminalSettings(_) = client
            .execute(
                client.prepare(Command::SaveTerminalSettings(Settings {
                    revision: settings.revision,
                    shell: "/usr/bin/false".into(),
                    environment: [
                        ("PATH".into(), login.second.to_str().unwrap().into()),
                        ("EXPLICIT_FIXTURE".into(), "configured".into()),
                    ]
                    .into(),
                })),
            )
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        let Output::TerminalTools(tools) = client
            .execute(client.prepare(Command::ListTerminalTools { worktree }))
            .await
            .unwrap()
        else {
            panic!("tools expected")
        };
        assert!(
            tools
                .iter()
                .any(|tool| tool.tool == Tool::Codex && tool.available)
        );
        open(&client, node.id(), worktree, "tool-ready:second:configured").await;
        assert_eq!(login.captures(), 1);
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let public = serde_json::to_string(&snapshot).unwrap();
        for secret in [
            "login-secret",
            "login-token",
            "ambient-model-secret",
            "configured",
        ] {
            assert!(!public.contains(secret));
        }
        let database = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        let leaked: i64 = database.query_row("SELECT count(*) FROM events WHERE CAST(body AS TEXT) LIKE '%login-secret%' OR CAST(body AS TEXT) LIKE '%ambient-model-secret%'", [], |row| row.get(0)).unwrap();
        assert_eq!(leaked, 0);
        drop(database);
        node.shutdown().await.unwrap();
        controller.shutdown().await.unwrap();
    }
}

#[tokio::test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn preserves_full_login_startup_and_workspace_home() {
    use sailry_node_runtime::EnvironmentFixture;
    use sailry_protocol::process::Access;
    use std::{ffi::OsStr, os::unix::fs::PermissionsExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().canonicalize().unwrap();
        let home = base.join("shell-home");
        let bin = home.join("bin");
        let root = base.join("project");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir(&root).unwrap();
        let shell = if cfg!(target_os = "macos") {
            "/bin/zsh"
        } else {
            "/bin/bash"
        };
        let node = Node::start_with_environment(
            base.join("node"),
            EnvironmentFixture {
                home: home.clone(),
                shell: shell.into(),
                environment: [
                    ("PATH".into(), "/usr/bin:/bin".into()),
                    ("LANG".into(), "C".into()),
                ]
                .into(),
                timeout: Duration::from_secs(5),
            },
        )
        .await
        .unwrap();
        // Install startup files after the Node's environment capture to prove PTY startup.
        let profile = "export SAILRY_LOGIN_MARKER=profile; export PATH=\"$HOME/bin:$PATH\"\n";
        let interactive = "export SAILRY_INTERACTIVE_MARKER=interactive; PS1='fixture> '\n";
        if cfg!(target_os = "macos") {
            std::fs::write(home.join(".zprofile"), profile).unwrap();
            std::fs::write(home.join(".zshrc"), interactive).unwrap();
            std::fs::write(home.join(".zlogin"), "export SAILRY_LOGIN_TAIL=tail\n").unwrap();
        } else {
            std::fs::write(
                home.join(".bash_profile"),
                format!("{profile}export SAILRY_LOGIN_TAIL=tail\n. \"$HOME/.bashrc\"\n"),
            )
            .unwrap();
            std::fs::write(home.join(".bashrc"), interactive).unwrap();
        }
        let program = bin.join("login-fixture-tool");
        std::fs::write(&program, "#!/bin/sh\nprintf 'login-program-ready\\n'\n").unwrap();
        std::fs::set_permissions(program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let controller = Link::controller(base.join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Login startup fixture".into(),
                path: root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let worktree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.project == Some(project.id))
            .unwrap()
            .id;
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(Launch {
                access: Access::Unrestricted,
                ..launch(worktree)
            })))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        command(&client, &info, "printf '%s' \"$HOME\" > terminal-home.txt; printf 'login:%s:%s:%s:%s\\n' \"$0\" \"$SAILRY_LOGIN_MARKER\" \"$SAILRY_INTERACTIVE_MARKER\" \"$SAILRY_LOGIN_TAIL\"; login-fixture-tool").await;
        until(&mut *stream, &mut projection, "login-program-ready").await;
        let output = content(&projection.snapshot().unwrap().screen);
        assert!(
            output.contains(if cfg!(target_os = "macos") {
                "login:-zsh:profile:interactive:tail"
            } else {
                "login:/bin/bash:profile:interactive:tail"
            }),
            "{output}"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("terminal-home.txt")).unwrap(),
            home.to_str().unwrap()
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let network = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let mut chunk = [0; 1024];
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0, "request ended before its headers");
                request.extend_from_slice(&chunk[..count]);
                assert!(
                    request.len() <= 16 * 1024,
                    "request headers exceed the fixture limit"
                );
            }
            assert!(request.starts_with(b"GET / HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 22\r\nConnection: close\r\n\r\nterminal-network-ready")
                .await
                .unwrap();
        });
        let curl = which::which_in("curl", Some(OsStr::new("/usr/bin:/bin")), &root)
            .expect("terminal network acceptance requires system curl");
        command(
            &client,
            &info,
            &format!(
                "'{}' --fail --silent --max-time 2 http://{address}/",
                curl.to_str().unwrap().replace('\'', "'\\''")
            ),
        )
        .await;
        until(&mut *stream, &mut projection, "terminal-network-ready").await;
        tokio::time::timeout(Duration::from_secs(5), network)
            .await
            .unwrap()
            .unwrap();
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            }))
            .await
            .unwrap();
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(launch(worktree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        command(&client, &info, "printf '%s' \"$HOME\" > workspace-home.txt; printf 'workspace-profile-%s\\n' \"${SAILRY_LOGIN_MARKER-unset}\"").await;
        until(&mut *stream, &mut projection, "workspace-profile-unset").await;
        assert_ne!(
            std::fs::read_to_string(root.join("workspace-home.txt")).unwrap(),
            home.to_str().unwrap()
        );
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            }))
            .await
            .unwrap();
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
