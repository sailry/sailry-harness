use super::*;

fn open(id: TerminalId, worktree: WorktreeId) -> Command {
    let launch = launch(worktree);
    Command::OpenTerminal {
        terminal: id,
        viewport: launch.viewport,
        appearance: launch.appearance,
    }
}

#[tokio::test]
async fn restores_explicit_shells() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Reopen fixture".into(),
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
    let mut originals = Vec::new();
    for _ in 0..3 {
        let Output::Terminal(info) = local
            .execute(local.prepare(Command::CreateTerminal(launch(tree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        originals.push(info);
    }
    local
        .execute(local.prepare(Command::CloseTerminal {
            worktree: tree,
            terminal: originals[2].id,
        }))
        .await
        .unwrap();
    node.shutdown().await.unwrap();

    let node = Node::start(&profile).await.unwrap();
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    // The remote controller opens first: desktop mounting must not be required.
    let remote = Client::new(controller.handle().remote(address));
    let local = Client::new(node.local());
    let clients = [&remote, &local];
    let Output::Terminals(stored) = local
        .execute(local.prepare(Command::ListTerminals { worktree: tree }))
        .await
        .unwrap()
    else {
        panic!("terminals expected")
    };
    for original in &originals[..2] {
        assert_eq!(
            stored
                .iter()
                .find(|info| info.id == original.id)
                .unwrap()
                .status,
            Status::Stopped
        );
        assert!(
            matches!(local.subscribe_terminal(original.id).await, Err(error) if error.code == ErrorCode::NotFound)
        );
    }
    for (index, client) in clients.into_iter().enumerate() {
        let original = &originals[index];
        let before = stored.iter().find(|info| info.id == original.id).unwrap();
        let request = client.prepare(open(original.id, tree));
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let output = admitted.completion.await.unwrap().unwrap();
        assert_eq!(client.execute(request).await.unwrap(), output);
        let Output::Terminal(info) = output else {
            panic!("terminal expected")
        };
        assert_eq!(info.id, original.id);
        assert_eq!(info.worktree, Some(tree));
        assert_eq!(info.status, Status::Running);
        assert!(info.revision > before.revision);
        // Another controller opening the same record neither restarts nor takes its lease.
        let Output::Terminal(reused) = clients[1 - index]
            .execute(clients[1 - index].prepare(open(info.id, tree)))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        assert_eq!(reused.id, info.id);
        assert_eq!(reused.revision, info.revision);
        assert_eq!(reused.owner, info.owner);
        let error = client
            .execute(client.prepare(Command::InputTerminal {
                terminal: info.id,
                revision: original.revision,
                input: Input::Text {
                    text: "stale".into(),
                },
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        let mut stream = client.subscribe_terminal(info.id).await.unwrap();
        let mut projection = Projection::new(node.id(), info.id, 1);
        command(client, &info, "printf '%s%s\\n' 'reopened-' 'shell'").await;
        until(&mut *stream, &mut projection, "reopened-shell").await;
    }
    assert_eq!(
        remote
            .execute(remote.prepare(open(originals[2].id, tree)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        remote
            .execute(remote.prepare(open(TerminalId::new(), tree)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let Output::Terminals(after) = local
        .execute(local.prepare(Command::ListTerminals { worktree: tree }))
        .await
        .unwrap()
    else {
        panic!("terminals expected")
    };
    assert_eq!(
        after.len(),
        3,
        "reopening must not create duplicate records"
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
