use super::*;
#[path = "../support/environment.rs"]
mod fixture;

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
