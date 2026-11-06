use super::*;

#[tokio::test]
async fn passes_arguments_on_local_and_remote_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("项目 with spaces");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("cwd-sentinel.txt"), "ready").unwrap();
    #[cfg(unix)]
    let script = directory.path().join("shell '资料 fixture.sh");
    #[cfg(windows)]
    let script = directory.path().join("shell 资料 fixture.ps1");
    #[cfg(unix)]
    let source = "if [ -f cwd-sentinel.txt ]; then printf 'arguments-ready:%s\\n' \"$1\"; fi; exec /bin/sh -i\n";
    #[cfg(windows)]
    let source =
        "if (Test-Path cwd-sentinel.txt) { Write-Output ('arguments-ready:' + $args[0]) }\r\n";
    std::fs::write(&script, source).unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
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
            name: "Shell argument fixture".into(),
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
    #[cfg(unix)]
    let shell = "/bin/sh".to_owned();
    #[cfg(windows)]
    let shell = {
        let Output::Shells(shells) = clients[0]
            .execute(clients[0].prepare(Command::ListShells))
            .await
            .unwrap()
        else {
            panic!("shells expected")
        };
        shells
            .into_iter()
            .find(|shell| {
                std::path::Path::new(&shell.path)
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case("powershell.exe"))
            })
            .expect("Windows PowerShell expected")
            .path
    };
    let literal = "argv 资料 with spaces & untouched";
    #[cfg(unix)]
    let arguments = vec![script.to_str().unwrap().into(), literal.into()];
    #[cfg(windows)]
    let arguments = vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-NoExit".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        script.to_str().unwrap().into(),
        literal.into(),
    ];
    for (index, client) in clients.iter().enumerate() {
        let Output::TerminalSettings(mut settings) = client
            .execute(client.prepare(Command::ReadTerminalSettings))
            .await
            .unwrap()
        else {
            panic!("settings expected")
        };
        settings.shell.clone_from(&shell);
        settings.shell_arguments.clone_from(&arguments);
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
        until(
            &mut *stream,
            &mut projection,
            &format!("arguments-ready:{literal}"),
        )
        .await;
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
