use std::time::Duration;

use sailry_client::{Client, FileView};
use sailry_link::{CancellationToken, Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{Command, ErrorCode, Output};
use tokio::sync::watch;

async fn wait(updates: &mut watch::Receiver<FileView>, check: impl Fn(&FileView) -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
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
    let node = Node::start(directory.path().join("node")).await.unwrap();
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
        wait(&mut updates, |view| view.revision > revision).await;
        revision = updates.borrow().revision;
        std::fs::rename(root.join("中文.txt"), root.join("renamed.txt")).unwrap();
        wait(&mut updates, |view| view.revision > revision).await;
        revision = updates.borrow().revision;
        std::fs::write(root.join("renamed.txt"), "updated").unwrap();
        wait(&mut updates, |view| view.revision > revision).await;
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
        wait(&mut updates, |view| view.revision > revision).await;
        stop.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!updates.borrow().connected);
    }
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM requests", [], |row| row
            .get::<_, i64>(0))
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
async fn reconnects_and_revokes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node");
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let node = Node::start(&path).await.unwrap();
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
    let node = Node::start_with_network(&path, NetworkScope::Direct(socket))
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
