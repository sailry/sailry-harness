use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::*;

async fn setup(client: &Client, path: &std::path::Path) -> WorktreeId {
    client
        .execute(client.prepare(Command::RegisterProject {
            name: "Fixture".into(),
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

fn save(worktree: WorktreeId, text: &str, expected: Option<&str>) -> Command {
    Command::WriteFile {
        worktree,
        path: "file.txt".into(),
        text: text.into(),
        expected_revision: expected.map(Into::into),
    }
}

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[tokio::test]
async fn local_and_remote() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let node = Node::start(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let worktree = setup(&local, &project).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invitation = node.link().invite().unwrap();
    let address = controller.handle().pair(invitation.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for (index, client) in [&local, &remote].into_iter().enumerate() {
        let nested = format!("output/{index}/reports/result.txt");
        let request = client.prepare(Command::WriteFile {
            worktree,
            path: nested.clone(),
            text: "created with parents".into(),
            expected_revision: None,
        });
        let saved = client.execute(request.clone()).await.unwrap();
        assert!(matches!(saved, Output::FileWritten(_)));
        assert_eq!(client.execute(request).await.unwrap(), saved);
        assert_eq!(
            std::fs::read_to_string(project.join(nested)).unwrap(),
            "created with parents"
        );
        let before = if index == 0 {
            None
        } else {
            Some(revision("external"))
        };
        let request = client.prepare(save(worktree, "new private content", before.as_deref()));
        let first = client.execute(request.clone()).await.unwrap();
        assert!(matches!(first, Output::FileWritten(_)));
        assert_eq!(
            std::fs::read_to_string(project.join("file.txt")).unwrap(),
            "new private content"
        );
        std::fs::write(project.join("file.txt"), "external").unwrap();
        assert_eq!(client.execute(request.clone()).await.unwrap(), first);
        assert_eq!(
            std::fs::read_to_string(project.join("file.txt")).unwrap(),
            "external"
        );
        let mut conflict = request;
        conflict.command = save(worktree, "different", before.as_deref());
        assert_eq!(
            client.execute(conflict).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(client.prepare(save(worktree, "stale", Some(&revision("old")))))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            client
                .execute(client.prepare(save(worktree, "overwrite", None)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
    let transport = node.local();
    let detached = local.prepare(save(worktree, "detached", Some(&revision("external"))));
    let admission = transport.dispatch(detached.clone()).await.unwrap();
    assert!(admission.receipt.durable);
    drop(admission.completion);
    // A storage snapshot is not a completion barrier for resource work.
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match local.execute(detached.clone()).await {
                Ok(Output::FileWritten(_)) => break,
                Err(error) if error.code == ErrorCode::OutcomeUnknown => {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                result => panic!("unexpected save result: {result:?}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(project.join("file.txt")).unwrap(),
        "detached"
    );
    let db = rusqlite::Connection::open(node.profile().join("storage/node.sqlite3")).unwrap();
    let stored: String = db
        .query_row(
            "SELECT group_concat(CAST(body AS TEXT)) FROM requests",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!stored.contains("private content"));
    drop(db);
    controller.close().await.unwrap();
    let admission = transport
        .dispatch(local.prepare(save(worktree, "shutdown", Some(&revision("detached")))))
        .await
        .unwrap();
    assert!(admission.receipt.durable);
    drop(admission.completion);
    node.shutdown().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(project.join("file.txt")).unwrap(),
        "shutdown"
    );
}

#[tokio::test]
async fn lost_result_is_not_replayed() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let profile = temp.path().join("node");
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let worktree = setup(&client, &project).await;
    let db = rusqlite::Connection::open(profile.join("storage/node.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER lose_file_result BEFORE UPDATE ON requests WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'injected result loss'); END;").unwrap();
    let request = client.prepare(save(worktree, "published", None));
    assert_eq!(
        client.execute(request.clone()).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        std::fs::read_to_string(project.join("file.txt")).unwrap(),
        "published"
    );
    db.execute_batch("DROP TRIGGER lose_file_result").unwrap();
    drop(db);
    node.shutdown().await.unwrap();
    std::fs::write(project.join("file.txt"), "external after crash").unwrap();
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    assert_eq!(
        client.execute(request).await.unwrap_err().code,
        ErrorCode::OutcomeUnknown
    );
    assert_eq!(
        std::fs::read_to_string(project.join("file.txt")).unwrap(),
        "external after crash"
    );
    node.shutdown().await.unwrap();
}
