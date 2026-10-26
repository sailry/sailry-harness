use std::time::Duration;

use sailry_client::{Client, FileView};
use sailry_link::{CancellationToken, Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, Output};
use tokio::sync::watch;

const NATIVE_WAIT: Duration = Duration::from_secs(if cfg!(target_os = "macos") { 30 } else { 8 });

async fn wait(updates: &mut watch::Receiver<FileView>, check: impl Fn(&FileView) -> bool) {
    wait_for(updates, Duration::from_secs(8), check).await;
}

async fn wait_for(
    updates: &mut watch::Receiver<FileView>,
    duration: Duration,
    check: impl Fn(&FileView) -> bool,
) {
    tokio::time::timeout(duration, async {
        loop {
            if check(&updates.borrow_and_update()) {
                return;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn shares_transient_invalidation() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Watched".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!()
    };
    let worktree = snapshot.worktrees[0].id;
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    for transport in [node.local(), controller.handle().remote(address)] {
        let client = Client::new(transport.clone());
        let stop = CancellationToken::new();
        let (sender, mut updates) = watch::channel(FileView::default());
        let cancelled = stop.clone();
        let task =
            tokio::spawn(async move { client.watch_files(worktree, sender, cancelled).await });
        wait(&mut updates, |view| view.connected).await;
        let mut revision = updates.borrow().revision;
        std::fs::write(root.join("中文.txt"), "created").unwrap();
        wait_for(&mut updates, NATIVE_WAIT, |view| view.revision > revision).await;
        revision = updates.borrow().revision;
        std::fs::rename(root.join("中文.txt"), root.join("renamed.txt")).unwrap();
        wait_for(&mut updates, NATIVE_WAIT, |view| view.revision > revision).await;
        revision = updates.borrow().revision;
        std::fs::write(root.join("renamed.txt"), "updated").unwrap();
        wait_for(&mut updates, NATIVE_WAIT, |view| view.revision > revision).await;
        let read = Client::new(transport);
        let Output::FileContent(content) = read
            .execute(read.prepare(Command::ReadFile {
                worktree,
                path: "renamed.txt".into(),
            }))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(content.text, "updated");
        revision = updates.borrow().revision;
        std::fs::remove_file(root.join("renamed.txt")).unwrap();
        wait_for(&mut updates, NATIVE_WAIT, |view| view.revision > revision).await;
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!updates.borrow().connected);
    }

    assert_eq!(
        {
            let sql = ("SELECT count(*) FROM requests").to_owned();
            let parameters = ();
            node.with_storage(move |db| {
                Box::pin(async move {
                    let row = db
                        .prepare(sql)
                        .await
                        .map_err(crate::database_fixture::fault)?
                        .query_row(parameters)
                        .await
                        .map_err(crate::database_fixture::fault)?;
                    let value: turso::Result<_> = row.get::<i64>(0);
                    value.map_err(crate::database_fixture::fault)
                })
            })
            .await
        }
        .unwrap(),
        1
    );
    let Output::Snapshot(after) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(after.cursor, snapshot.cursor);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn observes_repository_changes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    git2::Repository::init(&root).unwrap();
    std::fs::write(root.join("source.txt"), "initial").unwrap();
    let node = Node::start_empty(directory.path().join("node"))
        .await
        .unwrap();
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Repository".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected");
    };
    let worktree = snapshot.worktrees[0].id;
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    for (index, transport) in [node.local(), controller.handle().remote(address)]
        .into_iter()
        .enumerate()
    {
        let client = Client::new(transport.clone());
        let (sender, mut updates) = watch::channel(FileView::default());
        let stop = CancellationToken::new();
        let owner = Client::new(transport);
        let cancelled = stop.clone();
        let task =
            tokio::spawn(async move { owner.watch_repository(worktree, sender, cancelled).await });
        wait(&mut updates, |view| view.connected).await;
        let revision = updates.borrow().revision;
        let content = format!("external change {index}");
        std::fs::write(root.join("source.txt"), &content).unwrap();
        wait_for(&mut updates, NATIVE_WAIT, |view| view.revision > revision).await;
        let Output::FileContent(read) = client
            .execute(client.prepare(Command::ReadFile {
                worktree,
                path: "source.txt".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("file expected");
        };
        assert_eq!(read.text, content);
        stop.cancel();
        task.await.unwrap().unwrap();
        assert!(!updates.borrow().connected);
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn reconnects_and_revokes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node");
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start_empty(&path).await.unwrap();
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Watched".into(),
            path: root.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = local
        .execute(local.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!()
    };
    let worktree = snapshot.worktrees[0].id;
    let address = node.link().address();
    let socket = *address.ip_addrs().next().unwrap();
    let controller = Link::controller(directory.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let client = Client::new(controller.handle().remote(address));
    let (sender, mut updates) = watch::channel(FileView::default());
    let task = tokio::spawn(async move {
        client
            .watch_files(worktree, sender, CancellationToken::new())
            .await
    });
    wait(&mut updates, |view| view.connected).await;
    let revision = updates.borrow().revision;
    node.shutdown().await.unwrap();
    wait(&mut updates, |view| !view.connected && view.error.is_some()).await;
    std::fs::write(root.join("offline.txt"), "changed while disconnected").unwrap();
    let node = Node::start_empty_with_network(&path, NetworkScope::Direct(socket))
        .await
        .unwrap();
    wait(&mut updates, |view| {
        view.connected && view.revision > revision
    })
    .await;
    node.link()
        .set_trust(
            sailry_protocol::NodeId(*controller.handle().address().id.as_bytes()),
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(!updates.borrow().connected);
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
