use super::*;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn restores_execution_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project with spaces");
    let programs = directory.path().join("programs");
    let empty = directory.path().join("empty");
    for path in [&root, &programs, &empty] {
        std::fs::create_dir(path).unwrap();
    }
    let executable = programs.join("codex");
    std::fs::write(
        &executable,
        concat!(
            "#!/bin/sh\n",
            "printf 'started\\n' >> starts.txt\n",
            "printf 'tool-ready:%s:%s\\n' \"$PWD\" \"$CLI_FIXTURE\"\n",
            "while IFS= read -r line; do\n",
            "  printf 'tool-result:%s\\n' \"$line\"\n",
            "done\n",
        ),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let profile = directory.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let controller = Node::start(directory.path().join("controller"))
        .await
        .unwrap();
    let address = controller
        .link()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let local = Client::new(node.local());
    let remote = Client::new(controller.link().remote(address));
    let Output::Project(project) = local
        .execute(local.prepare(Command::RegisterProject {
            name: "CLI fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
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
    local
        .execute(
            local.prepare(Command::SaveTerminalSettings(Settings {
                revision: 0,
                shell: "/usr/bin/false".into(),
                environment: [
                    ("PATH".into(), programs.to_str().unwrap().into()),
                    ("CLI_FIXTURE".into(), "execution-node".into()),
                ]
                .into(),
            })),
        )
        .await
        .unwrap();
    let other = Client::new(controller.local());
    other
        .execute(other.prepare(Command::SaveTerminalSettings(Settings {
            revision: 0,
            shell: String::new(),
            environment: [("PATH".into(), empty.to_str().unwrap().into())].into(),
        })))
        .await
        .unwrap();
    let mut ids = Vec::new();
    for client in [&local, &remote] {
        let Output::TerminalTools(tools) = client
            .execute(client.prepare(Command::ListTerminalTools { worktree }))
            .await
            .unwrap()
        else {
            panic!("tools expected")
        };
        assert_eq!(
            tools
                .iter()
                .filter(|tool| tool.available)
                .map(|tool| tool.tool)
                .collect::<Vec<_>>(),
            vec![Tool::Codex]
        );
        assert_eq!(
            std::fs::read_to_string(root.join("starts.txt"))
                .unwrap_or_default()
                .lines()
                .count(),
            ids.len()
        );
        let request = client.prepare(Command::OpenToolTerminal {
            tool: Tool::Codex,
            launch: launch(worktree),
        });
        let Output::Terminal(info) = client.execute(request.clone()).await.unwrap() else {
            panic!("terminal expected")
        };
        assert_eq!(info.tool, Some(Tool::Codex));
        assert_eq!(info.ssh, None);
        let Output::Terminal(repeated) = client.execute(request).await.unwrap() else {
            panic!("terminal expected")
        };
        assert_eq!(repeated.id, info.id);
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut screen = Projection::new(node.id(), info.id, 1);
        until(&mut *stream, &mut screen, "execution-node").await;
        assert!(content(&screen.snapshot().unwrap().screen).contains(root.to_str().unwrap()));
        command(client, &info, "fixture-input").await;
        until(&mut *stream, &mut screen, "tool-result:fixture-input").await;
        drop(stream);
        let mut reconnected = client.subscribe_terminal(info.id).await.unwrap();
        let mut restored = Projection::new(node.id(), info.id, 1);
        until(
            &mut *reconnected,
            &mut restored,
            "tool-result:fixture-input",
        )
        .await;
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree,
                terminal: info.id,
            }))
            .await
            .unwrap();
        ids.push(info.id);
    }
    assert_eq!(
        std::fs::read_to_string(root.join("starts.txt")).unwrap(),
        "started\nstarted\n"
    );
    std::fs::rename(&executable, programs.join("removed")).unwrap();
    assert_eq!(
        remote
            .execute(remote.prepare(Command::OpenToolTerminal {
                tool: Tool::Codex,
                launch: launch(worktree)
            }))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotConfigured
    );
    node.shutdown().await.unwrap();
    let restored = Node::start(&profile).await.unwrap();
    let client = Client::new(restored.local());
    let Output::Terminals(terminals) = client
        .execute(client.prepare(Command::ListTerminals { worktree }))
        .await
        .unwrap()
    else {
        panic!("terminals expected")
    };
    assert_eq!(terminals.len(), 2);
    assert!(terminals.iter().all(|info| info.tool == Some(Tool::Codex)
        && info.status == Status::Closed
        && ids.contains(&info.id)));
    assert_eq!(
        std::fs::read_to_string(root.join("starts.txt")).unwrap(),
        "started\nstarted\n"
    );
    restored.shutdown().await.unwrap();
    controller.shutdown().await.unwrap();
}
