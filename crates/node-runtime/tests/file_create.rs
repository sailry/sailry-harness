use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

async fn register(client: &Client, path: &std::path::Path) -> WorktreeId {
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Creation fixture".into(),
            path: path.to_str().unwrap().into(),
        }))
        .await
        .unwrap();
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!()
    };
    snapshot.worktrees[0].id
}

#[tokio::test]
async fn shares_exclusive_creation() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let worktree = register(&local, &project).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        let path = format!("资料 {index}");
        let request = client.prepare(Command::CreateDirectory {
            worktree,
            path: path.clone(),
        });
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let result = admitted.completion.await.unwrap().unwrap();
        assert_eq!(result, Output::DirectoryCreated { path: path.clone() });
        assert!(project.join(&path).is_dir());
        // Simulate an external rename. Reusing the request must not create it again.
        std::fs::rename(project.join(&path), project.join(format!("moved-{index}"))).unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        assert!(!project.join(&path).exists());
        client
            .execute(client.prepare(Command::CreateDirectory {
                worktree,
                path: path.clone(),
            }))
            .await
            .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(Command::CreateDirectory {
                    worktree,
                    path: path.clone()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let file = format!("{path}/新文件.txt");
        client
            .execute(client.prepare(Command::WriteFile {
                worktree,
                path: file.clone(),
                text: String::new(),
                expected_revision: None,
            }))
            .await
            .unwrap();
        std::fs::write(project.join(&file), "keep").unwrap();
        assert_eq!(
            client
                .execute(client.prepare(Command::CreateDirectory {
                    worktree,
                    path: file.clone()
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            std::fs::read_to_string(project.join(&file)).unwrap(),
            "keep"
        );
        for path in ["", "../escape", "missing/child"] {
            assert!(
                client
                    .execute(client.prepare(Command::CreateDirectory {
                        worktree,
                        path: path.into()
                    }))
                    .await
                    .is_err()
            );
        }
        assert!(
            client
                .execute(client.prepare(Command::CreateDirectory {
                    worktree: WorktreeId::new(),
                    path: "wrong-owner".into()
                }))
                .await
                .is_err()
        );
        assert!(!project.join("wrong-owner").exists());
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_lost_completion() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let worktree = register(&client, &project).await;
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let request = client.prepare(Command::CreateDirectory {
        worktree,
        path: "created".into(),
    });
    assert_eq!(
        client.execute(request.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert!(project.join("created").is_dir());
    db.execute_batch("DROP TRIGGER lose_result").unwrap();
    drop(db);
    node.shutdown().await.unwrap();
    std::fs::rename(project.join("created"), project.join("moved")).unwrap();
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert!(!project.join("created").exists());
    node.shutdown().await.unwrap();
}
