use super::*;

fn update(expected: &Project, path: &Path) -> Command {
    Command::UpdateProject {
        expected: expected.clone(),
        name: expected.name.clone(),
        path: path.to_str().unwrap().into(),
        appearance: expected.appearance.clone(),
    }
}

#[tokio::test]
async fn guards_pending_open() {
    let (fixture, mut gate) = blocked().await;
    let (network, controller, remote) = connect(&fixture).await;
    for (index, client) in [&fixture.client, &remote].into_iter().enumerate() {
        let expected = snapshot(client).await.projects[0].clone();
        let path = fixture.directory.path().join(format!("rebound-{index}"));
        std::fs::create_dir(&path).unwrap();
        let first = fixture
            .client
            .dispatch(fixture.write("blocked", "preserved"))
            .await
            .unwrap();
        gate.entered.recv().await.unwrap();
        let opening = client
            .dispatch(client.prepare(Command::CreateTerminal(launch(fixture.worktree))))
            .await
            .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(update(&expected, &path)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy,
        );
        assert_eq!(snapshot(client).await.projects, vec![expected.clone()]);
        gate.release.send(()).unwrap();
        first.completion.await.unwrap().unwrap();
        let Output::Terminal(open) = opening.completion.await.unwrap().unwrap() else {
            panic!("terminal expected")
        };
        assert_eq!(open.status, terminal::Status::Running);
        assert_eq!(
            client
                .execute(client.prepare(update(&expected, &path)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy,
        );
        let Output::Terminal(closed) = client
            .execute(client.prepare(Command::CloseTerminal {
                worktree: fixture.worktree,
                terminal: open.id,
            }))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        assert_eq!(closed.status, terminal::Status::Closed);
        let Output::Project(updated) = client
            .execute(client.prepare(update(&expected, &path)))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        assert_eq!(updated.id, expected.id);
        assert_eq!(updated.path, path.to_str().unwrap());
        let state = snapshot(client).await;
        assert_eq!(
            state
                .terminals
                .iter()
                .find(|info| info.id == open.id)
                .unwrap()
                .status,
            terminal::Status::Closed
        );
        let main = state
            .worktrees
            .iter()
            .find(|tree| tree.id == fixture.worktree)
            .unwrap();
        assert_eq!(main.project, Some(expected.id));
        assert_eq!(main.path, updated.path);
        assert_eq!(
            std::fs::read_to_string(Path::new(&expected.path).join("blocked")).unwrap(),
            "preserved"
        );
    }
    controller.close().await.unwrap();
    network.close().await.unwrap();
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn guards_stopped_terminal() {
    let fixture = Fixture::start(mutations::execute).await;
    let expected = snapshot(&fixture.client).await.projects[0].clone();
    let path = fixture.directory.path().join("rebound");
    std::fs::create_dir(&path).unwrap();
    let mut info = saved(
        fixture.worktree,
        "00000000-0000-0000-0000-000000000001",
        terminal::Status::Stopped,
    );
    record(&fixture, info.clone()).await;
    assert_eq!(
        fixture
            .client
            .execute(fixture.client.prepare(update(&expected, &path)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::Busy,
    );
    info.status = terminal::Status::Closed;
    info.revision += 1;
    record(&fixture, info).await;
    assert!(matches!(
        fixture
            .client
            .execute(fixture.client.prepare(update(&expected, &path)))
            .await
            .unwrap(),
        Output::Project(_)
    ));
    fixture.store.shutdown().await.unwrap();
}
