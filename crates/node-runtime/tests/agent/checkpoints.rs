use super::*;
use serde_json::json;

#[path = "checkpoints/fixture.rs"]
mod fixture;
#[cfg(target_os = "macos")]
#[path = "checkpoints/native.rs"]
mod native;
#[path = "checkpoints/restore.rs"]
mod restore;

async fn list(
    client: &Client,
    session: SessionId,
    turn: TurnId,
    before: Option<CheckpointId>,
    limit: u16,
) -> checkpoint::Page {
    let Output::FileCheckpoints(page) = client
        .execute(client.prepare(Command::ListFileCheckpoints {
            session,
            turn,
            before,
            limit,
        }))
        .await
        .unwrap()
    else {
        panic!("checkpoints expected")
    };
    page
}

async fn read(
    client: &Client,
    session: SessionId,
    checkpoint: CheckpointId,
) -> checkpoint::Content {
    let Output::FileCheckpoint(content) = client
        .execute(client.prepare(Command::ReadFileCheckpoint {
            session,
            checkpoint,
        }))
        .await
        .unwrap()
    else {
        panic!("checkpoint content expected")
    };
    *content
}

#[tokio::test]
async fn captures_and_recovers() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let original = "Original 中文 🙂\n".repeat(4000);
        std::fs::write(root.join("existing.txt"), &original).unwrap();
        let node = Node::start(fixture.path().join("node")).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
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
        let first = "Created 中文 🙂";
        let second = "Edited 中文 🙂";
        let server = Server::tools(vec![
            (plugin_tool("files", "write_file"), json!({"path": "new.txt", "text": first, "expected_revision": null})),
            (plugin_tool("files", "write_file"), json!({"path": "new.txt", "text": second, "expected_revision": blake3::hash(first.as_bytes()).to_hex().to_string()})),
            (plugin_tool("files", "write_file"), json!({"path": "existing.txt", "text": "Replacement", "expected_revision": blake3::hash(original.as_bytes()).to_hex().to_string()})),
        ]).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let mut config = session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("session expected")
        };
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Make three file changes".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        let history = finished(&client, session.id, turn.id).await;
        assert_eq!(history.runs[0].status, Status::Completed);
        assert_eq!(
            std::fs::read_to_string(root.join("new.txt")).unwrap(),
            second
        );
        assert_eq!(
            std::fs::read_to_string(root.join("existing.txt")).unwrap(),
            "Replacement"
        );
        let mut files = Vec::new();
        let mut before = None;
        loop {
            let page = list(&client, session.id, turn.id, before, 1).await;
            files.extend(page.files);
            before = page.next;
            if before.is_none() {
                break;
            }
        }
        assert_eq!(files.len(), 3);
        let mut contents = Vec::new();
        for file in &files {
            let RequestOutcome::Completed(outcome) = &file.outcome else {
                panic!("durable outcome expected")
            };
            let Ok(Output::FileWritten(written)) = outcome.as_ref() else {
                panic!("write result expected")
            };
            assert_eq!(file.after.revision, written.revision);
            assert_eq!(file.after.size, written.size);
            assert_eq!(file.worktree, session.worktree);
            contents.push(read(&client, session.id, file.id).await);
        }
        assert_eq!(contents[0].before.as_deref(), Some(original.as_str()));
        assert_eq!(contents[0].after, "Replacement");
        assert_eq!(contents[1].before.as_deref(), Some(first));
        assert_eq!(contents[1].after, second);
        assert_eq!(contents[2].before, None);
        assert_eq!(contents[2].after, first);
        std::fs::write(root.join("new.txt"), "Later external content").unwrap();
        let Output::Session(branch) = client
            .execute(client.prepare(Command::ForkConversation {
                session: session.id,
                through: turn.id,
                expected_revision: session.revision,
            }))
            .await
            .unwrap()
        else {
            panic!("branch expected")
        };
        assert_eq!(
            list(&client, branch.id, turn.id, None, 100).await.files,
            files
        );
        let request = client.prepare(Command::RewindConversation {
            session: session.id,
            through: None,
            expected_head: turn.id,
            expected_revision: 1,
        });
        let result = client.execute(request.clone()).await.unwrap();
        let Output::Rewound(rewind) = &result else {
            panic!("rewind expected")
        };
        assert_eq!(
            list(&client, rewind.backup.id, turn.id, None, 100)
                .await
                .files,
            files
        );
        assert_eq!(
            client
                .execute(client.prepare(Command::ReadFileCheckpoint {
                    session: session.id,
                    checkpoint: files[0].id
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        assert_eq!(
            read(&client, branch.id, files[0].id).await.before,
            contents[0].before
        );
        assert_eq!(client.execute(request.clone()).await.unwrap(), result);
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions.len(), 3);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let node = Node::start(fixture.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(
            list(&client, branch.id, turn.id, None, 100).await.files,
            files
        );
        for content in &contents {
            let mut expected = content.clone();
            expected.session = branch.id;
            assert_eq!(read(&client, branch.id, content.file.id).await, expected);
        }
        assert_eq!(
            client
                .read_conversation(branch.id, None, 100)
                .await
                .unwrap()
                .page
                .entries,
            history.entries
        );
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        assert_eq!(
            std::fs::read_to_string(root.join("new.txt")).unwrap(),
            "Later external content"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("existing.txt")).unwrap(),
            "Replacement"
        );
        node.shutdown().await.unwrap();
    }
}
