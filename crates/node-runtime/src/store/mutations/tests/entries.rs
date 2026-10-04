use super::*;

#[derive(Clone, Copy)]
enum Change {
    Rename,
    Trash,
    Copy,
    CopyTo,
    MoveTo,
}

impl Change {
    fn command(self, worktree: WorktreeId, target: WorktreeId, from: &str, to: &str) -> Command {
        match self {
            Self::Rename => Command::RenameEntry {
                worktree,
                from: from.into(),
                to: to.into(),
            },
            Self::Copy => Command::CopyEntry {
                worktree,
                from: from.into(),
                to: to.into(),
            },
            Self::CopyTo => Command::CopyEntryTo {
                source: worktree,
                worktree: target,
                from: from.into(),
                to: to.into(),
            },
            Self::MoveTo => Command::MoveEntryTo {
                source: worktree,
                worktree: target,
                from: from.into(),
                to: to.into(),
            },
            Self::Trash => Command::TrashEntry {
                worktree,
                path: from.into(),
            },
        }
    }
}

#[tokio::test]
async fn reserves_registered_paths() {
    for change in [Change::Rename, Change::Copy, Change::CopyTo, Change::MoveTo] {
        reserves_paths(change).await;
    }
}

async fn reserves_paths(change: Change) {
    let (fixture, mut gate) = blocked().await;
    let root = fixture.directory.path().join("project");
    let (destination, target) = destination(&fixture, change).await;
    std::fs::create_dir(root.join("source")).unwrap();
    let first = fixture
        .client
        .dispatch(fixture.write("blocked", "first"))
        .await
        .unwrap();
    gate.entered.recv().await.unwrap();
    let request = fixture.client.prepare(change.command(
        fixture.worktree,
        target,
        if root.join("SOURCE").exists() {
            "SOURCE"
        } else {
            "source"
        },
        "target",
    ));
    let admission = fixture.client.dispatch(request.clone()).await.unwrap();
    assert!(admission.receipt.durable);
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
    for client in [&fixture.client, &remote] {
        for path in [root.join("source"), destination.join("target")] {
            let result = client
                .execute(client.prepare(Command::RegisterProject {
                    name: "Racing registration".into(),
                    path: path.to_str().unwrap().into(),
                }))
                .await;
            assert_eq!(result.unwrap_err().code, ErrorCode::Busy);
        }
        client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap();
        assert_eq!(
            fixture
                .client
                .execute(request.clone())
                .await
                .unwrap_err()
                .code,
            ErrorCode::OutcomeUnknown
        );
    }
    gate.release.send(()).unwrap();
    first.completion.await.unwrap().unwrap();
    admission.completion.await.unwrap().unwrap();
    assert_eq!(
        root.join("source").exists(),
        matches!(change, Change::Copy | Change::CopyTo)
    );
    fixture
        .client
        .execute(fixture.client.prepare(Command::RegisterProject {
            name: "After completion".into(),
            path: destination.join("target").to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    controller.close().await.unwrap();
    network.close().await.unwrap();
    fixture.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn validates_after_creation() {
    for change in [
        Change::Rename,
        Change::Trash,
        Change::Copy,
        Change::CopyTo,
        Change::MoveTo,
    ] {
        validates_creation(change).await;
    }
}

async fn validates_creation(change: Change) {
    let (entered, mut waiting) = mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    let fixture = Fixture::start(move |root, request| {
        if matches!(request.command, Command::CreateWorktree { .. }) {
            entered.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        mutations::execute(root, request)
    })
    .await;
    let root = fixture.directory.path().join("project");
    let (destination, target) = destination(&fixture, change).await;
    let repository = git2::Repository::init(&root).unwrap();
    let author = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let tree = repository
        .find_tree(repository.index().unwrap().write_tree().unwrap())
        .unwrap();
    let head = repository
        .commit(Some("HEAD"), &author, &author, "Fixture", &tree, &[])
        .unwrap();
    std::fs::create_dir(root.join("folder")).unwrap();
    let Output::Snapshot(snapshot) = fixture
        .client
        .execute(fixture.client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let create = fixture
        .client
        .dispatch(fixture.client.prepare(Command::CreateWorktree {
            project: snapshot.projects[0].id,
            path: root.join("folder/linked").to_str().unwrap().into(),
            branch: "linked".into(),
            commit: head.to_string(),
        }))
        .await
        .unwrap();
    waiting.recv().await.unwrap();
    let admission = fixture
        .client
        .dispatch(fixture.client.prepare(change.command(
            fixture.worktree,
            target,
            "folder",
            "moved",
        )))
        .await
        .unwrap();
    release.send(()).unwrap();
    create.completion.await.unwrap().unwrap();
    assert_eq!(
        admission.completion.await.unwrap().unwrap_err().code,
        ErrorCode::Conflict
    );
    assert!(root.join("folder/linked").is_dir());
    assert!(!destination.join("moved").exists());
    fixture.store.shutdown().await.unwrap();
}

async fn destination(fixture: &Fixture, change: Change) -> (std::path::PathBuf, WorktreeId) {
    if !matches!(change, Change::CopyTo | Change::MoveTo) {
        return (fixture.directory.path().join("project"), fixture.worktree);
    }
    let root = fixture.directory.path().join("destination");
    std::fs::create_dir(&root).unwrap();
    let Output::Project(project) = fixture
        .client
        .execute(fixture.client.prepare(Command::RegisterProject {
            name: "Destination".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap()
    else {
        panic!("project expected")
    };
    let Output::Snapshot(snapshot) = fixture
        .client
        .execute(fixture.client.prepare(Command::Snapshot))
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
    (root, worktree)
}
