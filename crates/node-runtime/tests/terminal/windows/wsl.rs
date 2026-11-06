use super::*;

#[tokio::test]
#[ignore = "requires Windows with WSL and an initialized Ubuntu distribution; SAILRY_WSL_DISTRIBUTION selects another installed distribution"]
async fn starts_selected_distribution_in_worktree() {
    let distribution = std::env::var("SAILRY_WSL_DISTRIBUTION").unwrap_or_else(|_| "Ubuntu".into());
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("项目 with spaces & 'quotes'");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("cwd-sentinel.txt"), "ready").unwrap();
    let node = Node::start_empty(temporary.path().join("node"))
        .await
        .unwrap();
    let controller = Link::controller(temporary.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let clients = [
        Client::new(node.local()),
        Client::new(controller.handle().remote(address)),
    ];
    let Output::Project(project) = clients[0]
        .execute(clients[0].prepare(Command::RegisterProject {
            name: "WSL terminal fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = clients[0]
        .execute(clients[0].prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let tree = snapshot
        .worktrees
        .iter()
        .find(|tree| tree.project == Some(project.id) && tree.main)
        .unwrap()
        .id;
    for (index, client) in clients.iter().enumerate() {
        let Output::Shells(shells) = client
            .execute(client.prepare(Command::ListShells))
            .await
            .unwrap()
        else {
            panic!("shells expected")
        };
        let selected = shells
            .into_iter()
            .find(|shell| {
                std::path::Path::new(&shell.path)
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case("wsl.exe"))
                    && shell.arguments == ["--distribution", distribution.as_str()]
            })
            .unwrap_or_else(|| panic!("installed WSL distribution {distribution:?} is required"));
        let Output::TerminalSettings(mut settings) = client
            .execute(client.prepare(Command::ReadTerminalSettings))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        settings.shell = selected.path;
        settings.shell_arguments = selected.arguments;
        client
            .execute(client.prepare(Command::SaveTerminalSettings(settings)))
            .await
            .unwrap();
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(Launch {
                access: sailry_protocol::process::Access::Unrestricted,
                ..launch(tree)
            })))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let mut stream = clients[1 - index]
            .subscribe_terminal(info.id)
            .await
            .unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        command(
            client,
            &info,
            "printf '%s:%s\\n' \"$WSL_DISTRO_NAME\" \"$(cat cwd-sentinel.txt)\"",
        )
        .await;
        until(
            &mut *stream,
            &mut projection,
            &format!("{distribution}:ready"),
        )
        .await;
        command(client, &info, "printf '%s-%s\\n' interactive ready").await;
        until(&mut *stream, &mut projection, "interactive-ready").await;
        client
            .execute(client.prepare(Command::CloseTerminal {
                worktree: tree,
                terminal: info.id,
            }))
            .await
            .unwrap();
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
