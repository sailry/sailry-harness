use super::*;
use git2::{BranchType, Repository, Signature};
use sailry_client::{Apply, Projection};
use sailry_link::{Link, NetworkScope};
use std::{path::Path, time::Duration};

pub(super) fn init(root: &Path) -> Repository {
    let repository = Repository::init(root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    std::fs::write(root.join("file"), "base").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("file")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    repository
        .commit(Some("HEAD"), &author, &author, "Fixture", &tree, &[])
        .unwrap();
    drop(tree);
    repository
}

fn command(project: ProjectId, destination: &Path, branch: &str, commit: git2::Oid) -> Command {
    Command::CreateWorktree {
        project,
        path: destination.to_str().unwrap().into(),
        branch: branch.into(),
        commit: commit.to_string(),
    }
}

#[tokio::test]
async fn local_and_remote_creation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = init(&root);
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let project = register(&local, &root).await;
    let other_root = temp.path().join("other");
    std::fs::create_dir(&other_root).unwrap();
    let other = register(&local, &other_root).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let advanced = repository
        .commit(
            Some("HEAD"),
            &author,
            &author,
            "Advanced",
            &base.tree().unwrap(),
            &[&base],
        )
        .unwrap();
    std::fs::write(root.join("file"), "uncommitted").unwrap();
    let index = std::fs::read(repository.path().join("index")).unwrap();
    for (sequence, client) in [&local, &remote].into_iter().enumerate() {
        let mut subscription = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        projection
            .apply(1, subscription.next().await.unwrap())
            .unwrap();
        let path = temp.path().join(format!("中文-worktree-{sequence}"));
        let name = format!("feature/中文-{sequence}");
        let request = client.prepare(command(project.id, &path, &name, base.id()));
        let admission = client.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        let output = admission.completion.await.unwrap().unwrap();
        let Output::Worktree(worktree) = &output else {
            panic!("worktree expected")
        };
        assert_eq!(worktree.project, Some(project.id));
        assert!(!worktree.main);
        assert_eq!(Path::new(&worktree.path), path.canonicalize().unwrap());
        let update = tokio::time::timeout(Duration::from_secs(5), subscription.next())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(&update, Update::Event(event) if event.event == Event::WorktreeRegistered(worktree.clone()))
        );
        assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
        assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
        assert!(projection.snapshot().unwrap().worktrees.contains(worktree));
        assert!(snapshot(client).await.worktrees.contains(worktree));
        let linked = Repository::open(&path).unwrap();
        assert_eq!(linked.head().unwrap().target(), Some(base.id()));
        assert_eq!(linked.head().unwrap().shorthand().unwrap(), name);
        assert_eq!(std::fs::read(path.join("file")).unwrap(), b"base");
        let Output::GitStatus(status) = client
            .execute(client.prepare(Command::InspectGit {
                worktree: worktree.id,
            }))
            .await
            .unwrap()
        else {
            panic!("Git status expected")
        };
        assert_eq!(status.branch.as_deref(), Some(name.as_str()));
        client
            .execute(client.prepare(Command::WriteFile {
                worktree: worktree.id,
                path: "file".into(),
                text: "linked edit".into(),
                expected_revision: Some(blake3::hash(b"base").to_hex().to_string()),
            }))
            .await
            .unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), output);
        assert_eq!(std::fs::read(path.join("file")).unwrap(), b"linked edit");
        let Output::Session(session) = client
            .execute(client.prepare(Command::CreateSession {
                project: Some(project.id),
                worktree: Some(worktree.id),
                config: Some(config()),
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(session.worktree, worktree.id);
        assert_eq!(
            client
                .execute(client.prepare(Command::CreateSession {
                    project: Some(other.id),
                    worktree: Some(worktree.id),
                    config: Some(config()),
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    assert_eq!(repository.head().unwrap().target(), Some(advanced));
    assert_eq!(
        std::fs::read(repository.path().join("index")).unwrap(),
        index
    );
    assert_eq!(std::fs::read(root.join("file")).unwrap(), b"uncommitted");
    let before = snapshot(&local).await;
    let profile = node.profile().to_owned();
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start(profile).await.unwrap();
    let after = snapshot(&Client::new(node.local())).await;
    assert_eq!(after.worktrees, before.worktrees);
    assert_eq!(after.sessions, before.sessions);
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_conflicting_entries() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = init(&root);
    let head = repository.head().unwrap().target().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let project = register(&local, &root).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let existing = temp.path().join("existing");
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("sentinel"), "preserve").unwrap();
    let fresh = temp.path().join("new");
    for client in [&local, &remote] {
        for (command, error) in [
            (
                command(project.id, &existing, "new", head),
                ErrorCode::Conflict,
            ),
            (
                command(project.id, &fresh, "main", head),
                ErrorCode::Conflict,
            ),
            (
                command(project.id, &fresh, "bad..name", head),
                ErrorCode::InvalidRequest,
            ),
            (
                command(project.id, Path::new("relative"), "new", head),
                ErrorCode::InvalidRequest,
            ),
            (
                command(project.id, &repository.path().join("nested"), "new", head),
                ErrorCode::InvalidRequest,
            ),
            (
                command(ProjectId::new(), &fresh, "new", head),
                ErrorCode::NotFound,
            ),
            (
                command(project.id, &temp.path().join("missing/new"), "new", head),
                ErrorCode::NotFound,
            ),
            (
                command(project.id, &fresh, "new", git2::Oid::ZERO_SHA1),
                ErrorCode::NotFound,
            ),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command))
                    .await
                    .unwrap_err()
                    .code,
                error
            );
        }
        assert!(repository.find_branch("new", BranchType::Local).is_err());
        assert!(!fresh.exists());
        assert_eq!(
            std::fs::read(existing.join("sentinel")).unwrap(),
            b"preserve"
        );
        assert_eq!(snapshot(client).await.worktrees.len(), 1);
    }
    let left = local.prepare(command(project.id, &fresh, "left", head));
    let right = remote.prepare(command(project.id, &fresh, "right", head));
    let (left, right) = tokio::join!(local.execute(left), remote.execute(right));
    assert_ne!(left.is_ok(), right.is_ok());
    assert_eq!(
        left.err().or(right.err()).unwrap().code,
        ErrorCode::Conflict
    );
    assert_eq!(snapshot(&local).await.worktrees.len(), 2);
    assert_ne!(
        repository.find_branch("left", BranchType::Local).is_ok(),
        repository.find_branch("right", BranchType::Local).is_ok()
    );
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_partial_creation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = init(&root);
    let head = repository.head().unwrap().target().unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let project = register(&local, &root).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    let before = snapshot(&local).await;
    for (sequence, client) in [&local, &remote].into_iter().enumerate() {
        let name = format!("partial-{sequence}");
        let path = temp.path().join(&name);
        let blocked = repository.path().join("worktrees");
        std::fs::write(&blocked, "fixture obstruction").unwrap();
        let request = client.prepare(command(project.id, &path, &name, head));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(repository.find_branch(&name, BranchType::Local).is_ok());
        assert!(!path.exists());
        assert_eq!(std::fs::read(&blocked).unwrap(), b"fixture obstruction");
        std::fs::remove_file(blocked).unwrap();
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert!(!path.exists());
    }
    assert_eq!(snapshot(&local).await, before);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovers_lost_result() {
    for remote in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repository");
        let repository = init(&root);
        let head = repository.head().unwrap().target().unwrap();
        let profile = temp.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
            .await
            .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let project = register(&client, &root).await;
        let before = snapshot(&client).await;
        let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
        let destination = temp.path().join("linked");
        let request = client.prepare(command(project.id, &destination, "linked", head));
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            Repository::open(&destination)
                .unwrap()
                .head()
                .unwrap()
                .target(),
            Some(head)
        );
        assert_eq!(snapshot(&client).await, before);
        db.execute_batch("DROP TRIGGER lose_result").unwrap();
        drop(db);
        node.shutdown().await.unwrap();
        std::fs::write(destination.join("file"), "later edit").unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read(destination.join("file")).unwrap(),
            b"later edit"
        );
        assert_eq!(snapshot(&client).await, before);
        let found = super::discovery::list(&client, before.worktrees[0].id).await;
        assert!(found.entries.iter().any(|entry| {
            Path::new(&entry.path) == destination.canonicalize().unwrap() && entry.available
        }));
        let recovered = super::discovery::adopt(&client, project.id, &destination).await;
        assert_eq!(snapshot(&client).await.worktrees.len(), 2);
        assert_eq!(
            super::discovery::adopt(&client, project.id, &destination).await,
            recovered
        );
        assert_eq!(
            client.execute(request.clone()).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        assert_eq!(
            std::fs::read(destination.join("file")).unwrap(),
            b"later edit"
        );
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert!(snapshot(&client).await.worktrees.contains(&recovered));
        assert_eq!(
            client.execute(request).await.unwrap_err().code,
            ErrorCode::OutcomeUnknown
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
