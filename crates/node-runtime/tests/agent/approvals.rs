use super::*;
use serde_json::json;

pub(super) async fn prepare(client: &Client, server: &Server, root: &std::path::Path) -> Session {
    let (session, mut provider) = configured(client, &server.endpoint, root).await;
    provider.default_model = "fixture-a".into();
    provider.models = vec![Model {
        id: "fixture-a".into(),
        context: 16000,
        output: 2048,
        vision: false,
        tools: true,
        reasoning: false,
        web_search: false,
        generates: vec![],
        efforts: Vec::new(),
        custom_efforts: false,
        default_effort: sailry_protocol::Effort::Default,
    }];
    client
        .execute(client.prepare(Command::PutProvider {
            provider,
            expected_revision: 1,
        }))
        .await
        .unwrap();
    session
}

pub(super) async fn pending(client: &Client, session: SessionId) -> (Page, Approval) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = history(client, session).await;
            if let Some(approval) = page
                .approvals
                .iter()
                .find(|approval| approval.state == ApprovalState::Pending)
                .cloned()
            {
                return (page, approval);
            }
            assert!(
                !page.runs.iter().any(|run| run.status == Status::Failed),
                "{:?}",
                page.runs
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("approval deadline")
}

pub(super) async fn submit(client: &Client, session: SessionId) -> TurnId {
    let Output::QueuedTurn(turn) = client
        .execute(client.prepare(Command::SubmitTurn {
            session,
            expected_revision: 1,
            message: "Write the requested file".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

#[tokio::test]
async fn resolves_exact_calls() {
    for remote in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("project");
        std::fs::create_dir(&root).unwrap();
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
            controller.handle().remote(address.clone())
        } else {
            node.local()
        });
        let server = Server::tools(vec![
            (
                plugin_tool("files", "write_file"),
                json!({"path": "资料.txt", "text": "first 中文 🙂", "expected_revision": null}),
            ),
            (
                plugin_tool("files", "write_file"),
                json!({"path": "资料.txt", "text": "must not replace", "expected_revision": null}),
            ),
            (
                plugin_tool("files", "write_file"),
                json!({"path": "denied.txt", "text": "denied", "expected_revision": null}),
            ),
        ])
        .await;
        let session = prepare(&client, &server, &root).await;
        let turn = submit(&client, session.id).await;
        let (page, first) = pending(&client, session.id).await;
        assert_eq!(page.runs[0].status, Status::Running);
        assert!(!root.join("资料.txt").exists());
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        let mut stream = client.subscribe_conversation(session.id).await.unwrap();
        let mut projection =
            sailry_client::conversation::Projection::new(client.target(), session.id, 0);
        assert_eq!(
            projection.apply(0, stream.next().await.unwrap()).unwrap(),
            sailry_client::Apply::Applied
        );
        assert_eq!(
            projection.snapshot().unwrap().page.approvals,
            vec![first.clone()]
        );
        let wrong = client.prepare(Command::ResolveApproval {
            session: SessionId::new(),
            approval: first.id,
            decision: Decision::Approve,
        });
        assert_eq!(
            client.execute(wrong).await.unwrap_err().code,
            ErrorCode::WrongTarget
        );
        drop(stream);
        drop(client);
        // A different authenticated controller can resume the Node-owned pending request.
        let other = Client::new(node.local());
        let request = other.prepare(Command::ResolveApproval {
            session: session.id,
            approval: first.id,
            decision: Decision::Approve,
        });
        let response = other.execute(request.clone()).await.unwrap();
        let (_, second) = pending(&other, session.id).await;
        assert_ne!(second.id, first.id);
        assert_eq!(
            std::fs::read_to_string(root.join("资料.txt")).unwrap(),
            "first 中文 🙂"
        );
        assert_eq!(other.execute(request.clone()).await.unwrap(), response);
        let resumed = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        resumed
            .execute(resumed.prepare(Command::ResolveApproval {
                session: session.id,
                approval: second.id,
                decision: Decision::Approve,
            }))
            .await
            .unwrap();
        let (_, third) = pending(&other, session.id).await;
        other
            .execute(other.prepare(Command::ResolveApproval {
                session: session.id,
                approval: third.id,
                decision: Decision::Deny,
            }))
            .await
            .unwrap();
        let page = finished(&other, session.id, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        assert_eq!(
            page.approvals
                .iter()
                .map(|approval| approval.state)
                .collect::<Vec<_>>(),
            vec![
                ApprovalState::Approved,
                ApprovalState::Approved,
                ApprovalState::Denied
            ]
        );
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::ToolResult { result, .. } => Some(result),
                _ => None,
            })
            .collect();
        assert_eq!(results.len(), 3);
        assert_eq!(results[1]["error"]["code"], "revision_conflict");
        assert!(!root.join("denied.txt").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("资料.txt")).unwrap(),
            "first 中文 🙂"
        );
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
        let node = Node::start(fixture.path().join("node")).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(history(&client, session.id).await, page);
        assert_eq!(client.execute(request).await.unwrap(), response);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn closes_interrupted_approvals() {
    for remote in [false, true] {
        for shutdown in [false, true] {
            let fixture = tempfile::tempdir().unwrap();
            let root = fixture.path().join("project");
            std::fs::create_dir(&root).unwrap();
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
            let server = Server::tools(vec![(
                plugin_tool("files", "write_file"),
                json!({"path": "never.txt", "text": "never", "expected_revision": null}),
            )])
            .await;
            let session = prepare(&client, &server, &root).await;
            let turn = submit(&client, session.id).await;
            let (_, approval) = pending(&client, session.id).await;
            if !shutdown {
                client
                    .execute(client.prepare(Command::StopTurn { turn }))
                    .await
                    .unwrap();
                let page = finished(&client, session.id, turn).await;
                assert_eq!(page.approvals[0].state, ApprovalState::Cancelled);
            }
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
            let node = Node::start(fixture.path().join("node")).await.unwrap();
            let client = Client::new(node.local());
            let page = history(&client, session.id).await;
            assert_eq!(
                page.approvals[0].state,
                if shutdown {
                    ApprovalState::Interrupted
                } else {
                    ApprovalState::Cancelled
                }
            );
            assert_eq!(
                client
                    .execute(client.prepare(Command::ResolveApproval {
                        session: session.id,
                        approval: approval.id,
                        decision: Decision::Approve
                    }))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
            assert!(!root.join("never.txt").exists());
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            node.shutdown().await.unwrap();
        }
    }
}
