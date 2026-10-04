use super::*;

#[tokio::test]
async fn presents_complete_frames() {
    let directory = tempfile::tempdir().unwrap();
    let script = directory.path().join("animation.sh");
    std::fs::write(
        &script,
        r#"stty -echo
printf '\033[2J\033[HREADY_FRAME\033[4;3H'
read token
printf '\033[?2026h\033[1;1HPARTIAL_FRAME\033[7;1Hfooter'
read token
printf '\033[1;1HFINAL_FRAME  \033[4;3H\033[?2026l'
read token
printf '\033[?2026h\033[2;1HTIMEOUT_FRAME'
read token
printf '\033[?2026h\033[3;1HEXIT_FRAME'
"#,
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
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Animation fixture".into(),
            path: directory.path().to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    local
        .execute(local.prepare(Command::SaveTerminalSettings(Settings {
            shell: "/bin/sh".into(),
            environment: [("HOME".into(), directory.path().to_str().unwrap().into())].into(),
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
    let tree = snapshot.worktrees[0].id;
    for transport in [node.local(), controller.handle().remote(address)] {
        let client = Client::new(transport);
        let Output::Terminal(info) = client
            .execute(client.prepare(Command::CreateTerminal(launch(tree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        command(&client, &info, "exec /bin/sh ./animation.sh").await;
        until(&mut *stream, &mut projection, "READY_FRAME").await;
        let before = projection.snapshot().unwrap().screen.clone();
        command(&client, &info, "go").await;
        assert!(
            tokio::time::timeout(Duration::from_millis(250), stream.next())
                .await
                .is_err(),
            "incomplete frame was published"
        );
        let mut resumed = client.subscribe_terminal(info.id).await.unwrap();
        let mut restored = Projection::new(node.id(), info.id, 2);
        restored.apply(2, resumed.next().await.unwrap()).unwrap();
        assert_eq!(restored.snapshot().unwrap().screen, before);
        command(&client, &info, "go").await;
        until(&mut *stream, &mut projection, "FINAL_FRAME").await;
        assert_eq!(projection.snapshot().unwrap().screen.cursor, before.cursor);
        assert!(!content(&projection.snapshot().unwrap().screen).contains("PARTIAL_FRAME"));
        command(&client, &info, "go").await;
        until(&mut *stream, &mut projection, "TIMEOUT_FRAME").await;
        command(&client, &info, "go").await;
        until(&mut *stream, &mut projection, "EXIT_FRAME").await;
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
