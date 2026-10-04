#![cfg(unix)]
use super::*;
use process::{Fixture, decide, results};
use sailry_protocol::process::Outcome;
use serde_json::json;

async fn configure(fixture: &mut Fixture, permission: Permission) {
    let mut config = fixture.session.config.clone();
    config.permission = permission;
    let Output::Session(session) = fixture
        .client
        .execute(fixture.client.prepare(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            config,
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    fixture.session = session;
}

async fn submit(fixture: &Fixture) -> (Request, QueuedTurn) {
    let request = fixture.client.prepare(Command::SubmitTurn {
        session: fixture.session.id,
        expected_revision: fixture.session.revision,
        message: "Use the requested tools".into(),
    });
    let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap() else {
        panic!("turn expected")
    };
    (request, turn)
}

#[tokio::test]
async fn permits_reads() {
    for remote in [false, true] {
        for permission in [Permission::Ask, Permission::Project, Permission::Full] {
            let server = Server::tools(vec![
                (
                    plugin_tool("files", "read_file"),
                    json!({"path": "read.txt"}),
                ),
                (
                    plugin_tool("files", "search_files"),
                    json!({"query": "readable"}),
                ),
            ])
            .await;
            let mut fixture = Fixture::new(remote, &server).await;
            std::fs::write(fixture.root.join("read.txt"), "readable").unwrap();
            configure(&mut fixture, permission).await;
            let (_, turn) = submit(&fixture).await;
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            assert!(page.approvals.is_empty());
            assert_eq!(server.requests.lock().unwrap().len(), 3);
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn freezes_queued_writes() {
    for remote in [false, true] {
        for permission in [Permission::Project, Permission::Full] {
            let server = Server::tools(vec![
                (plugin_tool("files", "write_file"), json!({"path": "资料.txt", "text": "approved by policy 🙂", "expected_revision": null})),
            ]).await;
            let mut fixture = Fixture::new(remote, &server).await;
            configure(&mut fixture, permission).await;
            let request = fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Write the requested file".into(),
            });
            let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
            else {
                panic!("turn expected")
            };
            configure(&mut fixture, Permission::Ask).await;
            let controller = Client::new(fixture.node.local());
            controller
                .execute(controller.prepare(Command::StartQueuedTurn { turn: turn.id }))
                .await
                .unwrap();
            let page = finished(&controller, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            assert_eq!(page.runs[0].revision, turn.revision);
            assert_eq!(page.approvals.len(), 1);
            assert_eq!(page.approvals[0].state, ApprovalState::Approved);
            assert_eq!(
                page.approvals[0].source,
                if permission == Permission::Project {
                    ApprovalSource::Project
                } else {
                    ApprovalSource::Full
                }
            );
            assert_eq!(
                std::fs::read_to_string(fixture.root.join("资料.txt")).unwrap(),
                "approved by policy 🙂"
            );
            assert_eq!(
                fixture.client.execute(request).await.unwrap(),
                Output::QueuedTurn(turn)
            );
            fixture.node.shutdown().await.unwrap();
            fixture
                .controller
                .handle()
                .disconnect(fixture.client.target())
                .await;
            let reopened = Node::start(fixture.directory.path().join("node"))
                .await
                .unwrap();
            let client = Client::new(if remote {
                fixture
                    .controller
                    .handle()
                    .remote(reopened.link().address())
            } else {
                reopened.local()
            });
            assert_eq!(history(&client, fixture.session.id).await, page);
            assert_eq!(server.requests.lock().unwrap().len(), 2);
            reopened.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn confirms_project_commands() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (
                plugin_tool("files", "write_file"),
                json!({"path": "inside.txt", "text": "automatic", "expected_revision": null}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"command": "printf x >> count.txt; printf allowed > ../outside.txt"}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"command": "printf denied > ../denied.txt"}),
            ),
        ])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        configure(&mut fixture, Permission::Project).await;
        let (request, turn) = submit(&fixture).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert_eq!(approval.source, ApprovalSource::User);
        assert!(!fixture.directory.path().join("outside.txt").exists());
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("inside.txt")).unwrap(),
            "automatic"
        );
        assert!(!fixture.root.join("count.txt").exists());
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        fixture
            .controller
            .handle()
            .disconnect(fixture.node.id())
            .await;
        let resumed = Client::new(
            fixture
                .controller
                .handle()
                .remote(fixture.node.link().address()),
        );
        assert_eq!(
            approvals::pending(&resumed, fixture.session.id).await.1,
            approval
        );
        let (decision, receipt) = decide(&resumed, &approval, Decision::Approve).await;
        let (_, denied) = approvals::pending(&resumed, fixture.session.id).await;
        assert_eq!(denied.source, ApprovalSource::User);
        decide(&resumed, &denied, Decision::Deny).await;
        let page = finished(&resumed, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(
            page.approvals
                .iter()
                .map(|item| item.source)
                .collect::<Vec<_>>(),
            vec![
                ApprovalSource::Project,
                ApprovalSource::User,
                ApprovalSource::User
            ]
        );
        let output = results(&page);
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].outcome, Outcome::Exited(0));
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("outside.txt")).unwrap(),
            "allowed"
        );
        assert!(!fixture.directory.path().join("denied.txt").exists());
        assert_eq!(resumed.execute(decision).await.unwrap(), receipt);
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("count.txt")).unwrap(),
            "x"
        );
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn permits_authorized_commands() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            plugin_tool("commands", "run_command"),
            json!({"command": "printf x >> ../outside.txt"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        configure(&mut fixture, Permission::Full).await;
        let (request, turn) = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.approvals[0].source, ApprovalSource::Full);
        assert_eq!(results(&page)[0].outcome, Outcome::Exited(0));
        assert_eq!(
            fixture.client.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        assert_eq!(
            std::fs::read_to_string(fixture.directory.path().join("outside.txt")).unwrap(),
            "x"
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
