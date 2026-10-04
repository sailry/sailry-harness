use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

#[tokio::test]
async fn refreshes_pages_without_persistence() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join("资料")).unwrap();
    for index in (0..1200).rev() {
        std::fs::write(project.join(format!("{index:04}.txt")), "fixture").unwrap();
    }
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    local
        .execute(local.prepare(Command::RegisterProject {
            name: "Fixture".into(),
            path: project.to_str().unwrap().into(),
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
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invite = node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let transport = controller.handle().remote(address);
    let remote = Client::new(transport.clone());
    for client in [&local, &remote] {
        let first = client.list_directory(worktree, "", None).await.unwrap();
        assert_eq!(first.entries.len(), MAX_DIRECTORY_ENTRIES);
        assert_eq!(first.entries[0].name, "资料");
        let second = client
            .list_directory(worktree, "", Some(&first))
            .await
            .unwrap();
        assert_eq!(second.entries.len(), 1000);
        let third = client
            .list_directory(worktree, "", Some(&second))
            .await
            .unwrap();
        assert_eq!(third.entries.len(), 1201);
        assert!(!third.truncated);
        assert!(third.next.is_none());
        assert_eq!(third.entries.last().unwrap().name, "1199.txt");
        let refreshed = client.refresh_directory(worktree, "", 1000).await.unwrap();
        assert_eq!(refreshed, second);
        assert_eq!(
            client
                .list_directory(WorktreeId::new(), "", None)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(
            client
                .list_directory(worktree, "../", None)
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert!(
            client
                .list_directory(worktree, "资料", None)
                .await
                .unwrap()
                .entries
                .is_empty()
        );

        std::fs::rename(project.join("1199.txt"), project.join("renamed.txt")).unwrap();
        assert_eq!(
            client
                .list_directory(worktree, "", Some(&first))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        let changed = client
            .refresh_directory(worktree, "", third.entries.len())
            .await
            .unwrap();
        assert!(
            changed
                .entries
                .iter()
                .any(|entry| entry.name == "renamed.txt")
        );
        assert!(!changed.entries.iter().any(|entry| entry.name == "1199.txt"));
        std::fs::rename(project.join("renamed.txt"), project.join("1199.txt")).unwrap();
    }
    let admission = transport
        .dispatch(remote.prepare(Command::ListDirectory {
            worktree,
            path: String::new(),
            after: None,
        }))
        .await
        .unwrap();
    assert!(!admission.receipt.durable);
    admission.completion.await.unwrap().unwrap();
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1, "only registration is durable");
    drop(db);
    node.link()
        .set_trust(NodeId(*controller.handle().address().id.as_bytes()), false)
        .await
        .unwrap();
    assert!(remote.list_directory(worktree, "", None).await.is_err());
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    assert!(local.list_directory(worktree, "", None).await.is_err());
}
