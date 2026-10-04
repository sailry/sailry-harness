use super::*;
use question::{Answer, Response, State};
use sailry_protocol::conversation::search::Query;
use serde_json::json;

async fn submit(client: &Client, session: &Session, message: &str) -> TurnId {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

async fn revise(client: &Client, session: &Session, permission: Permission) -> Session {
    let mut config = session.config.clone();
    config.permission = permission;
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
    session
}

#[tokio::test]
async fn resumes_without_tool_replay() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let profile = fixture.path().join("node");
        let node = Node::start(&profile).await.unwrap();
        let controller =
            Link::controller(fixture.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let transport = if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        };
        let client = Client::new(transport.clone());
        let server = Server::tools(vec![
            (plugin_tool("files", "write_file"), json!({"path": "fork.txt", "text": "Original write 中文 🙂", "expected_revision": null})),
            ("ask_user".into(), json!({"prompt": "Describe the task", "input": {"kind": "text", "multiline": true, "max_bytes": 1024}})),
        ]).await;
        let session = approvals::prepare(&client, &server, &root).await;
        let session = revise(&client, &session, Permission::Full).await;
        let first = submit(&client, &session, "Original first 中文 🙂").await;
        let question = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let page = history(&client, session.id).await;
                if let Some(question) = page
                    .questions
                    .iter()
                    .find(|question| question.state == State::Pending)
                {
                    break question.clone();
                }
                assert!(
                    !page.runs.iter().any(|run| run.status == Status::Failed),
                    "{page:?}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            client
                .execute(client.prepare(Command::ForkConversation {
                    session: session.id,
                    through: first,
                    expected_revision: session.revision,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Busy
        );
        client
            .execute(client.prepare(Command::ResolveQuestion {
                session: session.id,
                question: question.id,
                response: Response::Answer(Answer::Text("Chosen 中文 🙂".into())),
            }))
            .await
            .unwrap();
        let original = finished(&client, session.id, first).await;
        assert_eq!(original.runs[0].status, Status::Completed);
        assert_eq!(original.entries.len(), 8);
        assert_eq!(original.approvals.len(), 1);
        assert_eq!(original.approvals[0].state, ApprovalState::Approved);
        assert_eq!(original.questions.len(), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 3);

        let session = revise(&client, &session, Permission::Ask).await;
        let queued = client.prepare(Command::QueueTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Keep this pending".into(),
        });
        client.execute(queued).await.unwrap();
        let request = client.prepare(Command::ForkConversation {
            session: session.id,
            through: first,
            expected_revision: session.revision,
        });
        // Discard the application completion after durable receipt, then retry the same fork.
        let admission = transport.dispatch(request.clone()).await.unwrap();
        assert!(admission.receipt.durable);
        drop(admission);
        let Output::Session(branch) = client.execute(request.clone()).await.unwrap() else {
            panic!("fork expected")
        };
        assert_ne!(branch.id, session.id);
        assert_eq!(branch.config, session.config);
        assert_eq!(branch.revision, 1);
        assert_eq!(
            branch.fork,
            Some(Fork {
                session: session.id,
                through: first
            })
        );
        assert_eq!(
            client.execute(request.clone()).await.unwrap(),
            Output::Session(branch.clone())
        );
        let mut altered = request.clone();
        if let Command::ForkConversation {
            expected_revision, ..
        } = &mut altered.command
        {
            *expected_revision = 1;
        }
        assert_eq!(
            client.execute(altered).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        let inherited = client
            .read_conversation(branch.id, None, 20)
            .await
            .unwrap()
            .page;
        assert_eq!(inherited.entries, original.entries);
        assert_eq!(inherited.runs[0].origin, Some(session.id));
        assert_eq!(inherited.runs[0].revision, 2);
        assert_eq!(inherited.runs[0].session, branch.id);
        assert!(inherited.queue.items.is_empty());
        assert_eq!(inherited.approvals[0].id, original.approvals[0].id);
        assert_eq!(inherited.approvals[0].session, branch.id);
        assert_eq!(inherited.questions[0].id, original.questions[0].id);
        assert_eq!(inherited.questions[0].session, branch.id);
        assert_eq!(
            client
                .execute(client.prepare(Command::ResolveQuestion {
                    session: branch.id,
                    question: question.id,
                    response: Response::Cancel,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::WrongTarget
        );
        let matches = client
            .search_conversation(
                branch.id,
                Query {
                    text: "Original first".into(),
                    case_sensitive: true,
                    before: None,
                    limit: 7,
                },
            )
            .await
            .unwrap();
        assert_eq!(matches.matches[0].turn, first);
        assert_eq!(matches.matches[0].entry, original.entries[0].id);
        let mut updates = client.subscribe_conversation(branch.id).await.unwrap();
        let mut projection =
            sailry_client::conversation::Projection::new(client.target(), branch.id, 0);
        projection.apply(0, updates.next().await.unwrap()).unwrap();
        assert_eq!(*projection.snapshot().unwrap().page, inherited);

        // Another Client resumes the branch's own configuration, independent of the source queue.
        let other = Client::new(node.local());
        let next = submit(&other, &branch, "Branch continuation").await;
        let branched = finished(&other, branch.id, next).await;
        assert_eq!(branched.runs.len(), 2);
        assert_eq!(branched.runs.last().unwrap().status, Status::Completed);
        assert_eq!(branched.runs.last().unwrap().origin, None);
        assert_eq!(branched.entries.len(), 10);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        let body = server.requests.lock().unwrap().last().unwrap().clone();
        let contents: Vec<_> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_str())
            .collect();
        assert!(contents.contains(&"Original first 中文 🙂"));
        assert!(contents.contains(&"Branch continuation"));
        assert!(!contents.contains(&"Keep this pending"));
        let unchanged = history(&client, session.id).await;
        assert_eq!(unchanged.entries, original.entries);
        assert_eq!(unchanged.queue.items.len(), 1);
        assert_eq!(
            std::fs::read_to_string(root.join("fork.txt")).unwrap(),
            "Original write 中文 🙂"
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            while projection
                .snapshot()
                .unwrap()
                .page
                .runs
                .last()
                .unwrap()
                .status
                != Status::Completed
                || projection.snapshot().unwrap().page.runs.len() != 2
            {
                projection.apply(0, updates.next().await.unwrap()).unwrap();
            }
        })
        .await
        .unwrap();
        assert_eq!(*projection.snapshot().unwrap().page, branched);
        drop(updates);
        node.shutdown().await.unwrap();
        let node = Node::start(&profile).await.unwrap();
        let restored = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(
            restored.execute(request).await.unwrap(),
            Output::Session(branch.clone())
        );
        assert_eq!(
            restored
                .read_conversation(branch.id, None, 100)
                .await
                .unwrap()
                .page,
            branched
        );
        assert_eq!(history(&restored, session.id).await, unchanged);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
