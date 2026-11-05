use super::*;

#[tokio::test]
async fn starts_cmd_in_selected_worktree() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("项目 with spaces");
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
    let local = &clients[0];
    let Output::Project(project) = local
        .execute(local.prepare(Command::RegisterProject {
            name: "Windows terminal fixture".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Shells(shells) = local
        .execute(local.prepare(Command::ListShells))
        .await
        .unwrap()
    else {
        panic!("shells expected")
    };
    let shell = shells
        .into_iter()
        .find(|shell| {
            std::path::Path::new(shell)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
        })
        .expect("Windows command interpreter expected");
    local
        .execute(local.prepare(Command::SaveTerminalSettings(Settings {
            shell,
            ..Default::default()
        })))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
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
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(launch(tree))))
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
            r#"for %M in (ready) do @if /I "%CD%"=="%PWD%" if exist cwd-sentinel.txt echo worktree-%M"#,
        )
        .await;
        until(&mut *stream, &mut projection, "worktree-ready").await;
        drop(stream);
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
