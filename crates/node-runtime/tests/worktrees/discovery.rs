use super::creation::init;
use super::*;
use git2::Repository;
use sailry_client::{Apply, Projection};
use sailry_link::{Link, NetworkScope};
use std::{path::Path, time::Duration};

pub(super) async fn list(client: &Client, worktree: WorktreeId) -> GitWorktrees {
    let admission = client
        .dispatch(client.prepare(Command::ListWorktrees { worktree }))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    let Output::GitWorktrees(value) = admission.completion.await.unwrap().unwrap() else {
        panic!("worktree listing expected")
    };
    value
}

fn command(project: ProjectId, path: &Path) -> Command {
    Command::RegisterWorktree {
        project,
        path: path.to_str().unwrap().into(),
    }
}

pub(super) async fn adopt(client: &Client, project: ProjectId, path: &Path) -> Worktree {
    let admission = client
        .dispatch(client.prepare(command(project, path)))
        .await
        .unwrap();
    assert!(admission.receipt.durable);
    let Output::Worktree(value) = admission.completion.await.unwrap().unwrap() else {
        panic!("worktree expected")
    };
    value
}

#[tokio::test]
async fn local_and_remote_discovery() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = init(&root);
    let head = repository.head().unwrap().target().unwrap().to_string();
    let linked = temp.path().join("中文-worktree");
    repository
        .worktree("feature", &linked, None)
        .unwrap()
        .lock(Some("Fixture"))
        .unwrap();
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
    let main = &before.worktrees[0];
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    let count = || {
        db.query_row("SELECT count(*) FROM requests", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    };
    let initial_count = count();
    let expected = GitWorktrees {
        kind: RepositoryKind::Ready,
        entries: vec![
            GitWorktree {
                path: main.path.clone(),
                branch: Some("main".into()),
                head: Some(head.clone()),
                main: true,
                locked: false,
                available: true,
            },
            GitWorktree {
                path: linked.canonicalize().unwrap().to_str().unwrap().into(),
                branch: Some("feature".into()),
                head: Some(head),
                main: false,
                locked: true,
                available: true,
            },
        ],
        truncated: false,
        omitted_paths: 0,
    };
    for client in [&local, &remote] {
        assert_eq!(list(client, main.id).await, expected);
        assert_eq!(snapshot(client).await, before);
    }
    assert_eq!(count(), initial_count);
    let mut subscription = remote.subscribe().await.unwrap();
    let mut projection = Projection::new(node.id(), 1);
    projection
        .apply(1, subscription.next().await.unwrap())
        .unwrap();
    let (first, second) = tokio::join!(
        adopt(&local, project.id, &linked),
        adopt(&remote, project.id, &linked)
    );
    assert_eq!(first, second);
    assert!(!first.main);
    let update = tokio::time::timeout(Duration::from_secs(5), subscription.next())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(&update, Update::Event(event) if event.event == Event::WorktreeRegistered(first.clone()))
    );
    assert_eq!(projection.apply(1, update.clone()).unwrap(), Apply::Applied);
    assert_eq!(projection.apply(1, update).unwrap(), Apply::Ignored);
    let registered = snapshot(&local).await;
    assert_eq!(registered.worktrees.len(), 2);
    assert_eq!(projection.snapshot().unwrap(), &registered);
    for client in [&local, &remote] {
        assert_eq!(adopt(client, project.id, &root).await, *main);
        assert_eq!(adopt(client, project.id, &linked).await, first);
        assert_eq!(list(client, first.id).await, expected);
        assert_eq!(snapshot(client).await, registered);
        let Output::FileContent(file) = client
            .execute(client.prepare(Command::ReadFile {
                worktree: first.id,
                path: "file".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("file expected")
        };
        assert_eq!(file.text, "base");
    }
    let Output::Session(session) = remote
        .execute(remote.prepare(Command::CreateSession {
            project: Some(project.id),
            worktree: Some(first.id),
            config: Some(config()),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    let state = snapshot(&local).await;
    let parked = temp.path().join("parked");
    std::fs::rename(&linked, &parked).unwrap();
    for client in [&local, &remote] {
        let missing = list(client, main.id).await;
        assert_eq!(missing.entries.len(), 2);
        let entry = &missing.entries[1];
        assert!(!entry.available);
        assert!(entry.locked);
        assert_eq!(entry.path, first.path);
        assert_eq!(snapshot(client).await, state);
        assert_eq!(
            client
                .execute(client.prepare(command(project.id, &linked)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
    std::fs::rename(&parked, &linked).unwrap();
    assert_eq!(adopt(&remote, project.id, &linked).await, first);
    assert_eq!(list(&local, main.id).await, expected);
    drop(db);
    let profile = node.profile().to_owned();
    node.shutdown().await.unwrap();
    let node = Node::start(profile).await.unwrap();
    let resumed = snapshot(&Client::new(
        controller.handle().remote(node.link().address()),
    ))
    .await;
    assert_eq!(resumed.worktrees, state.worktrees);
    assert!(resumed.sessions.contains(&session));
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_foreign_contexts() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let repository = init(&root);
    let foreign = temp.path().join("foreign");
    init(&foreign);
    let linked = temp.path().join("linked");
    repository.worktree("linked", &linked, None).unwrap();
    let alias = temp.path().join("alias");
    std::fs::create_dir(&alias).unwrap();
    std::fs::copy(linked.join(".git"), alias.join(".git")).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let project = register(&local, &root).await;
    let linked_project = register(&local, &linked).await;
    let before = snapshot(&local).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&local, &remote] {
        for (path, code) in [
            (&foreign, ErrorCode::Conflict),
            (&linked, ErrorCode::Conflict),
            (&alias, ErrorCode::InvalidRequest),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(command(project.id, path)))
                    .await
                    .unwrap_err()
                    .code,
                code,
                "unexpected registration result for {}",
                path.display()
            );
        }
        assert_eq!(
            client
                .execute(client.prepare(command(project.id, Path::new("relative"))))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client
                .execute(client.prepare(command(ProjectId::new(), &linked)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ListWorktrees {
                    worktree: WorktreeId::new()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let own = adopt(client, linked_project.id, &linked).await;
        assert!(own.main);
        assert_eq!(own.project, Some(linked_project.id));
        let found = list(client, own.id).await;
        assert!(found.entries[0].main);
        assert_eq!(
            found.entries[0].path,
            root.canonicalize().unwrap().to_str().unwrap()
        );
        assert!(!found.entries[1].main);
        assert_eq!(snapshot(client).await, before);
    }
    assert_eq!(std::fs::read(linked.join("file")).unwrap(), b"base");
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn empty_and_detached_states() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let client = Client::new(node.local());
    register(&client, &root).await;
    let before = snapshot(&client).await;
    let main = before.worktrees[0].id;
    let directory = list(&client, main).await;
    assert_eq!(directory.kind, RepositoryKind::Directory);
    assert!(directory.entries.is_empty());
    let repository = Repository::init(&root).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    let unborn = list(&client, main).await;
    assert_eq!(unborn.kind, RepositoryKind::Unborn);
    assert!(unborn.entries[0].available);
    assert_eq!(unborn.entries[0].branch.as_deref(), Some("main"));
    assert!(unborn.entries[0].head.is_none());
    let repository = init(&root);
    let head = repository.head().unwrap().target().unwrap();
    repository.set_head_detached(head).unwrap();
    let detached = list(&client, main).await;
    assert_eq!(detached.kind, RepositoryKind::Ready);
    assert!(detached.entries[0].branch.is_none());
    assert_eq!(detached.entries[0].head, Some(head.to_string()));
    assert_eq!(snapshot(&client).await, before);
    node.shutdown().await.unwrap();
}
