use super::*;

#[tokio::test]
#[cfg(unix)]
async fn restores_reported_activity() {
    let directory = tempfile::tempdir().unwrap();
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
            name: "Activity fixture".into(),
            path: directory.path().to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let mut settings = Settings {
        shell: "/bin/sh".into(),
        ..Default::default()
    };
    settings
        .environment
        .insert("HOME".into(), directory.path().to_str().unwrap().into());
    local
        .execute(local.prepare(Command::SaveTerminalSettings(settings)))
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
        assert_eq!(info.activity, None);
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        for (sequence, activity) in [
            ("133;C", Activity::Idle),
            ("9;4;3", Activity::Working),
            ("9;4;0", Activity::Idle),
            ("9;4;1;50", Activity::Working),
            ("9;4;4", Activity::WaitingForInput),
            ("9;4;2", Activity::Failed),
            ("133;D;0", Activity::Idle),
            ("9;4;3", Activity::Working),
            ("133;A", Activity::Idle),
        ] {
            command(&client, &info, &format!("printf '\\033]{sequence}\\007'")).await;
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    projection.apply(1, stream.next().await.unwrap()).unwrap();
                    if projection.snapshot().is_some_and(|snapshot| {
                        snapshot
                            .info
                            .activity
                            .is_some_and(|report| report.state == activity)
                    }) {
                        break;
                    }
                }
            })
            .await
            .expect("activity update deadline");
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            let current = snapshot
                .terminals
                .iter()
                .find(|value| value.id == info.id)
                .unwrap();
            assert_eq!(current.activity.unwrap().state, activity);
            assert_eq!(current.revision, info.revision);
            assert_eq!(current.status, Status::Running);
            let mut resumed = client.subscribe_terminal(info.id).await.unwrap();
            let mut restored = Projection::new(node.id(), info.id, 2);
            restored.apply(2, resumed.next().await.unwrap()).unwrap();
            assert_eq!(
                restored.snapshot().unwrap().info.activity.unwrap().state,
                activity
            );
        }
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
