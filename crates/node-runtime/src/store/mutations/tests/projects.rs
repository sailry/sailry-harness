use super::super::super::Job;
use super::*;

async fn snapshot(client: &Client) -> Snapshot {
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    snapshot
}

fn launch(worktree: WorktreeId) -> terminal::Launch {
    terminal::Launch {
        access: process::Access::Workspace,
        worktree,
        viewport: terminal::Viewport {
            columns: 80,
            rows: 24,
            pixel_width: 640,
            pixel_height: 480,
        },
        appearance: terminal::Appearance {
            foreground: terminal::Rgb {
                red: 240,
                green: 240,
                blue: 240,
            },
            background: terminal::Rgb {
                red: 20,
                green: 20,
                blue: 20,
            },
            palette: [terminal::Rgb {
                red: 128,
                green: 128,
                blue: 128,
            }; 16],
            color_scheme: terminal::ColorScheme::Dark,
        },
    }
}

fn saved(worktree: WorktreeId, id: &str, status: terminal::Status) -> terminal::Info {
    terminal::Info {
        id: id.parse().unwrap(),
        worktree: Some(worktree),
        ssh: None,
        tool: None,
        access: Some(process::Access::Workspace),
        status,
        activity: None,
        title: None,
        directory: None,
        owner: None,
        revision: 1,
    }
}

async fn record(fixture: &Fixture, info: terminal::Info) {
    fixture
        .store
        .ingress
        .sender
        .send(Job::TerminalInfo(info))
        .await
        .unwrap();
    snapshot(&fixture.client).await;
}

async fn connect(fixture: &Fixture) -> (Link, Link, Client) {
    let network = Link::bind(
        &fixture.identity,
        NetworkScope::default(),
        fixture.store.ingress.clone(),
        fixture.store.ingress.clone(),
    )
    .await
    .unwrap();
    let controller = Link::controller(
        fixture.directory.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let address = controller
        .handle()
        .pair(network.handle().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    (network, controller, remote)
}

#[tokio::test]
async fn closes_earlier_admission() {
    let (fixture, mut gate) = blocked().await;
    let (network, controller, remote) = connect(&fixture).await;
    let expected = snapshot(&fixture.client).await.projects[0].clone();
    let stopped = saved(
        fixture.worktree,
        "00000000-0000-0000-0000-000000000001",
        terminal::Status::Stopped,
    );
    record(&fixture, stopped.clone()).await;
    let first = fixture
        .client
        .dispatch(fixture.write("blocked", "retained"))
        .await
        .unwrap();
    gate.entered.recv().await.unwrap();
    let earlier = fixture
        .client
        .dispatch(
            fixture
                .client
                .prepare(Command::CreateTerminal(launch(fixture.worktree))),
        )
        .await
        .unwrap();
    let request = fixture.client.prepare(Command::RemoveProject {
        expected: expected.clone(),
    });
    let removal = fixture.client.dispatch(request.clone()).await.unwrap();
    assert!(removal.receipt.durable);
    for client in [&fixture.client, &remote] {
        let appearance = launch(fixture.worktree);
        for command in [
            Command::CreateSession {
                project: Some(expected.id),
                worktree: None,
                config: None,
            },
            Command::CreateTerminal(launch(fixture.worktree)),
            Command::OpenTerminal {
                terminal: stopped.id,
                viewport: appearance.viewport,
                appearance: appearance.appearance,
            },
            Command::RegisterWorktree {
                project: expected.id,
                path: expected.path.clone(),
            },
            Command::UpdateProject {
                expected: expected.clone(),
                name: "Changed".into(),
                path: expected.path.clone(),
                appearance: expected.appearance.clone(),
            },
            Command::RemoveProject {
                expected: expected.clone(),
            },
        ] {
            let error = client.execute(client.prepare(command)).await.unwrap_err();
            assert_eq!(error.code, ErrorCode::Busy);
            assert_eq!(error.details, None);
        }
    }
    let nested = fixture.directory.path().join("project/nested");
    std::fs::create_dir(&nested).unwrap();
    let Output::Project(other) = remote
        .execute(remote.prepare(Command::RegisterProject {
            name: "Distinct nested project".into(),
            path: nested.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let other_tree = snapshot(&remote)
        .await
        .worktrees
        .into_iter()
        .find(|tree| tree.project == Some(other.id))
        .unwrap()
        .id;
    let unaffected = remote
        .dispatch(remote.prepare(Command::CreateTerminal(launch(other_tree))))
        .await
        .unwrap();
    gate.release.send(()).unwrap();
    first.completion.await.unwrap().unwrap();
    let Output::Terminal(earlier) = earlier.completion.await.unwrap().unwrap() else {
        panic!("terminal expected")
    };
    assert_eq!(
        removal.completion.await.unwrap().unwrap(),
        Output::ProjectRemoved { id: expected.id }
    );
    assert_eq!(
        fixture.client.execute(request).await.unwrap(),
        Output::ProjectRemoved { id: expected.id }
    );
    let Output::Terminal(unaffected) = unaffected.completion.await.unwrap().unwrap() else {
        panic!("terminal expected")
    };
    let state = snapshot(&remote).await;
    for id in [stopped.id, earlier.id] {
        assert_eq!(
            state
                .terminals
                .iter()
                .find(|info| info.id == id)
                .unwrap()
                .status,
            terminal::Status::Closed
        );
    }
    assert_eq!(
        state
            .terminals
            .iter()
            .find(|info| info.id == unaffected.id)
            .unwrap()
            .status,
        terminal::Status::Running
    );
    assert_eq!(state.projects, vec![other]);
    assert_eq!(
        std::fs::read_to_string(fixture.directory.path().join("project/blocked")).unwrap(),
        "retained"
    );
    controller.close().await.unwrap();
    network.close().await.unwrap();
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn retains_partial_failure() {
    let fixture = Fixture::start(mutations::execute).await;
    let (network, controller, remote) = connect(&fixture).await;
    let expected = snapshot(&fixture.client).await.projects[0].clone();
    // This ID sorts after every v4 live terminal, so its failure follows a real close.
    let missing = saved(
        fixture.worktree,
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
        terminal::Status::Running,
    );
    record(&fixture, missing.clone()).await;
    for (index, client) in [&fixture.client, &remote].into_iter().enumerate() {
        let Output::Terminal(live) = client
            .execute(client.prepare(Command::CreateTerminal(launch(fixture.worktree))))
            .await
            .unwrap()
        else {
            panic!("terminal expected")
        };
        let request = client.prepare(Command::RemoveProject {
            expected: expected.clone(),
        });
        let error = client.execute(request.clone()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(
            error.details,
            Some(FaultDetails::ProjectRemoval {
                tasks_active: false,
                terminals_may_be_closed: true,
            })
        );
        assert_eq!(client.execute(request.clone()).await.unwrap_err(), error);
        assert_eq!(
            client.outcome(&request).await.unwrap(),
            RequestOutcome::Completed(Box::new(Err(error)))
        );
        let state = snapshot(client).await;
        assert_eq!(state.projects, vec![expected.clone()]);
        assert_eq!(
            state
                .terminals
                .iter()
                .find(|info| info.id == live.id)
                .unwrap()
                .status,
            terminal::Status::Closed
        );
        assert_eq!(
            state
                .terminals
                .iter()
                .find(|info| info.id == missing.id)
                .unwrap()
                .status,
            terminal::Status::Running
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::InspectTerminal { terminal: live.id }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let path = format!("after-failure-{index}");
        client
            .execute(fixture.write(&path, "preserved"))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("project").join(path)).unwrap(),
            "preserved"
        );
    }
    controller.close().await.unwrap();
    network.close().await.unwrap();
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_unavailable_service() {
    let fixture = Fixture::start(mutations::execute).await;
    let expected = snapshot(&fixture.client).await.projects[0].clone();
    let stopped = saved(
        fixture.worktree,
        "00000000-0000-0000-0000-000000000001",
        terminal::Status::Stopped,
    );
    record(&fixture, stopped.clone()).await;
    fixture.store.ingress.terminals.stop();
    let request = fixture.client.prepare(Command::RemoveProject {
        expected: expected.clone(),
    });
    let error = fixture.client.execute(request.clone()).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(
        error.details,
        Some(FaultDetails::ProjectRemoval {
            tasks_active: false,
            terminals_may_be_closed: true,
        })
    );
    assert_eq!(
        fixture.client.execute(request.clone()).await.unwrap_err(),
        error
    );
    assert_eq!(
        fixture.client.outcome(&request).await.unwrap(),
        RequestOutcome::Completed(Box::new(Err(error)))
    );
    let state = snapshot(&fixture.client).await;
    assert_eq!(state.projects, vec![expected]);
    assert_eq!(state.terminals, vec![stopped]);
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn rechecks_new_tasks() {
    let (fixture, mut gate) = blocked().await;
    let expected = snapshot(&fixture.client).await.projects[0].clone();
    let Output::Session(session) = fixture
        .client
        .execute(fixture.client.prepare(Command::CreateSession {
            project: Some(expected.id),
            worktree: None,
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::High,
                mode: WorkMode::Code,
                permission: Permission::Ask,
                credential: None,
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    let Output::Terminal(live) = fixture
        .client
        .execute(
            fixture
                .client
                .prepare(Command::CreateTerminal(launch(fixture.worktree))),
        )
        .await
        .unwrap()
    else {
        panic!("terminal expected")
    };
    let first = fixture
        .client
        .dispatch(fixture.write("blocked", "retained"))
        .await
        .unwrap();
    gate.entered.recv().await.unwrap();
    let removal = fixture
        .client
        .dispatch(fixture.client.prepare(Command::RemoveProject {
            expected: expected.clone(),
        }))
        .await
        .unwrap();
    fixture
        .client
        .execute(fixture.client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Hold the isolated task".into(),
        }))
        .await
        .unwrap();
    gate.release.send(()).unwrap();
    first.completion.await.unwrap().unwrap();
    let error = removal.completion.await.unwrap().unwrap_err();
    assert_eq!(error.code, ErrorCode::Busy);
    assert_eq!(
        error.details,
        Some(FaultDetails::ProjectRemoval {
            tasks_active: true,
            terminals_may_be_closed: false,
        })
    );
    let state = snapshot(&fixture.client).await;
    assert_eq!(state.projects, vec![expected]);
    assert_eq!(
        state
            .terminals
            .iter()
            .find(|info| info.id == live.id)
            .unwrap()
            .status,
        terminal::Status::Running
    );
    fixture.store.shutdown().await.unwrap();
}
