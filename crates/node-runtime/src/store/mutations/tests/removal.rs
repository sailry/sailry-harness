use super::*;

fn session(worktree: &Worktree) -> Command {
    Command::CreateSession {
        project: worktree.project,
        worktree: Some(worktree.id),
        config: Some(SessionConfig {
            assistant: None,
            resource: None,
            provider: ProviderId::new(),
            model: "test".into(),
            effort: Effort::High,
            mode: sailry_protocol::WorkMode::Code,
            permission: sailry_protocol::Permission::Ask,
            credential: None,
        }),
    }
}

async fn linked(fixture: &Fixture) -> (Worktree, Command) {
    let root = fixture.directory.path().join("project");
    let repository = git2::Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    let tree = repository
        .find_tree(repository.index().unwrap().write_tree().unwrap())
        .unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let head = repository
        .commit(Some("HEAD"), &author, &author, "Fixture", &tree, &[])
        .unwrap();
    let Output::Snapshot(snapshot) = fixture
        .client
        .execute(fixture.client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let Output::Worktree(worktree) = fixture
        .client
        .execute(
            fixture.client.prepare(Command::CreateWorktree {
                project: snapshot.projects[0].id,
                path: fixture
                    .directory
                    .path()
                    .join("linked")
                    .to_str()
                    .unwrap()
                    .into(),
                branch: "linked".into(),
                commit: head.to_string(),
            }),
        )
        .await
        .unwrap()
    else {
        panic!("worktree expected")
    };
    let remove = Command::RemoveWorktree {
        worktree: worktree.id,
        expected_head: head.to_string(),
        expected_branch: "linked".into(),
    };
    (worktree, remove)
}

#[tokio::test]
async fn blocks_new_owners() {
    let (entered, mut waiting) = mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    let fixture = Fixture::start(move |root, request| {
        if matches!(request.command, Command::RemoveWorktree { .. }) {
            entered.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        mutations::execute(root, request)
    })
    .await;
    let (worktree, remove) = linked(&fixture).await;
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
    let request = fixture.client.prepare(remove.clone());
    let admission = fixture.client.dispatch(request.clone()).await.unwrap();
    waiting.recv().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        for client in [&fixture.client, &remote] {
            for command in [
                session(&worktree),
                Command::WriteFile {
                    worktree: worktree.id,
                    path: "late".into(),
                    text: "preserve".into(),
                    expected_revision: None,
                },
                Command::InspectGit {
                    worktree: worktree.id,
                },
                Command::CopyEntryTo {
                    source: worktree.id,
                    worktree: fixture.worktree,
                    from: "source".into(),
                    to: "copy".into(),
                },
                Command::CopyEntryTo {
                    source: fixture.worktree,
                    worktree: worktree.id,
                    from: "source".into(),
                    to: "copy".into(),
                },
                Command::RegisterProject {
                    name: "Nested".into(),
                    path: worktree.path.clone(),
                },
                Command::MoveEntryTo {
                    source: worktree.id,
                    worktree: fixture.worktree,
                    from: "source".into(),
                    to: "moved".into(),
                },
                Command::MoveEntryTo {
                    source: fixture.worktree,
                    worktree: worktree.id,
                    from: "source".into(),
                    to: "moved".into(),
                },
                Command::RegisterWorktree {
                    project: worktree.project.unwrap(),
                    path: worktree.path.clone(),
                },
                remove.clone(),
            ] {
                assert_eq!(
                    client
                        .execute(client.prepare(command))
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::Busy
                );
            }
            client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap();
        }
        assert_eq!(
            fixture.client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            remote
                .execute(remote.prepare(remove.clone()))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
    })
    .await
    .unwrap();
    release.send(()).unwrap();
    assert_eq!(
        admission.completion.await.unwrap().unwrap(),
        Output::WorktreeRemoved { id: worktree.id }
    );
    assert!(!Path::new(&worktree.path).exists());
    assert_eq!(
        remote
            .execute(remote.prepare(session(&worktree)))
            .await
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    controller.close().await.unwrap();
    network.close().await.unwrap();
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn respects_earlier_writes() {
    let (fixture, mut gate) = blocked().await;
    let (worktree, remove) = linked(&fixture).await;
    let write = fixture
        .client
        .dispatch(fixture.client.prepare(Command::WriteFile {
            worktree: worktree.id,
            path: "blocked".into(),
            text: "preserve".into(),
            expected_revision: None,
        }))
        .await
        .unwrap();
    gate.entered.recv().await.unwrap();
    let removal = fixture
        .client
        .dispatch(fixture.client.prepare(remove))
        .await
        .unwrap();
    gate.release.send(()).unwrap();
    write.completion.await.unwrap().unwrap();
    assert_eq!(
        removal.completion.await.unwrap().unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        std::fs::read(Path::new(&worktree.path).join("blocked")).unwrap(),
        b"preserve"
    );
    // A rejected removal must release its ownership guard.
    fixture
        .client
        .execute(fixture.client.prepare(Command::ReadFile {
            worktree: worktree.id,
            path: "blocked".into(),
        }))
        .await
        .unwrap();
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn respects_earlier_registrations() {
    let (entered, mut waiting) = mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    let fixture = Fixture::start(move |root, request| {
        if matches!(&request.command, Command::CreateWorktree { branch, .. } if branch == "nested")
        {
            entered.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        mutations::execute(root, request)
    })
    .await;
    let (tree, remove) = linked(&fixture).await;
    std::fs::write(
        fixture.directory.path().join("project/.git/info/exclude"),
        "nested/\n",
    )
    .unwrap();
    let nested = Path::new(&tree.path).join("nested");
    let Command::RemoveWorktree {
        ref expected_head, ..
    } = remove
    else {
        unreachable!()
    };
    let create = fixture
        .client
        .dispatch(fixture.client.prepare(Command::CreateWorktree {
            project: tree.project.unwrap(),
            path: nested.to_str().unwrap().into(),
            branch: "nested".into(),
            commit: expected_head.clone(),
        }))
        .await
        .unwrap();
    waiting.recv().await.unwrap();
    let removal = fixture
        .client
        .dispatch(fixture.client.prepare(remove))
        .await
        .unwrap();
    release.send(()).unwrap();
    create.completion.await.unwrap().unwrap();
    assert_eq!(
        removal.completion.await.unwrap().unwrap_err().code,
        ErrorCode::Conflict
    );
    assert!(nested.join(".git").is_file());
    fixture.store.shutdown().await.unwrap();
}
