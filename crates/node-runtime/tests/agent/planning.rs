use super::*;
use process::Fixture;
use serde_json::{Value, json};

#[path = "planning/review.rs"]
mod review;

pub(super) async fn configure(fixture: &mut Fixture, mode: WorkMode) {
    let mut config = fixture.session.config.clone();
    config.mode = mode;
    config.permission = Permission::Full;
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

fn tool_names(request: &Value) -> Vec<&str> {
    request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn preserves_queued_write_authority() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            plugin_tool("files", "write_file"),
            json!({
                "path": "code.txt", "text": "Frozen coding turn", "expected_revision": null
            }),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        configure(&mut fixture, WorkMode::Code).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Write the requested file".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        configure(&mut fixture, WorkMode::Plan).await;
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StartQueuedTurn { turn: turn.id }),
            )
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].source, ApprovalSource::Full);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("code.txt")).unwrap(),
            "Frozen coding turn"
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        for request in requests {
            assert!(tool_names(&request).contains(&plugin_tool("files", "write_file").as_str()));
            assert!(
                !request["messages"]
                    .to_string()
                    .contains("This is a planning turn")
            );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn freezes_child_mode() {
    for remote in [false, true] {
        let server = Server::parallel(vec![(
            crate::agent_support::plugin_tool("delegation", "spawn_agent"),
            json!({
                "role": null, "task": "Inspect the task and propose a plan"
            }),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        configure(&mut fixture, WorkMode::Plan).await;
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Delegate the independent analysis".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("turn expected")
        };
        configure(&mut fixture, WorkMode::Code).await;
        fixture
            .client
            .execute(
                fixture
                    .client
                    .prepare(Command::StartQueuedTurn { turn: turn.id }),
            )
            .await
            .unwrap();
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let Output::Snapshot(snapshot) = fixture
            .client
            .execute(fixture.client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        let child = snapshot
            .sessions
            .iter()
            .find(|session| session.delegation.is_some())
            .unwrap();
        assert_eq!(child.config.mode, WorkMode::Plan);
        assert_eq!(child.config.permission, Permission::Full);
        assert!(
            history(&fixture.client, child.id)
                .await
                .approvals
                .is_empty()
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        for request in requests {
            let names = tool_names(&request);
            assert!(
                !names.contains(&plugin_tool("files", "write_file").as_str())
                    && !names.contains(&plugin_tool("commands", "run_command").as_str())
            );
            for name in ["create_worktree", "register_worktree", "remove_worktree"] {
                assert!(!names.contains(&plugin_tool("worktrees", name).as_str()));
            }
            assert!(
                request["messages"]
                    .to_string()
                    .contains("This is a planning turn")
            );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn restores_frozen_tools() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path": "read.txt"}),
        )])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        std::fs::write(fixture.root.join("read.txt"), "Planning input 中文 🙂").unwrap();
        configure(&mut fixture, WorkMode::Plan).await;
        let request = fixture.client.prepare(Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Inspect the project and propose a plan".into(),
        });
        let Output::QueuedTurn(turn) = fixture.client.execute(request.clone()).await.unwrap()
        else {
            panic!("turn expected")
        };
        assert_eq!(turn.config.mode, WorkMode::Plan);
        configure(&mut fixture, WorkMode::Code).await;
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
        resumed
            .execute(resumed.prepare(Command::StartQueuedTurn { turn: turn.id }))
            .await
            .unwrap();
        let page = finished(&resumed, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.runs[0].revision, turn.revision);
        assert!(page.approvals.is_empty());
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            let names = tool_names(request);
            for name in [
                plugin_tool("files", "read_file").as_str(),
                plugin_tool("files", "search_files").as_str(),
                plugin_tool("files", "list_directory").as_str(),
                plugin_tool("git", "git_status").as_str(),
                plugin_tool("git", "git_diff").as_str(),
                plugin_tool("git", "git_log").as_str(),
                plugin_tool("worktrees", "list_worktrees").as_str(),
                "ask_user",
                plugin_tool("delegation", "spawn_agent").as_str(),
            ] {
                assert!(names.contains(&name), "missing {name}: {names:?}");
            }
            assert!(
                !names.contains(&plugin_tool("files", "write_file").as_str())
                    && !names.contains(&plugin_tool("commands", "run_command").as_str())
            );
            for name in ["create_worktree", "register_worktree", "remove_worktree"] {
                assert!(!names.contains(&plugin_tool("worktrees", name).as_str()));
            }
            assert!(
                request["messages"]
                    .to_string()
                    .contains("This is a planning turn")
            );
        }
        assert!(
            requests[1]["messages"]
                .to_string()
                .contains("Planning input 中文 🙂")
        );
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(fixture.directory.path().join("node"))
            .await
            .unwrap();
        let reopened = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&reopened, fixture.session.id).await, page);
        assert_eq!(
            reopened.execute(request).await.unwrap(),
            Output::QueuedTurn(turn)
        );
        let Output::Snapshot(snapshot) = reopened
            .execute(reopened.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert_eq!(snapshot.sessions[0].config.mode, WorkMode::Code);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_mutations_before_approval() {
    for remote in [false, true] {
        for (name, args) in [
            (
                plugin_tool("files", "write_file"),
                json!({"path": "forbidden.txt", "text": "changed", "expected_revision": null}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"command": "printf changed > forbidden.txt"}),
            ),
        ] {
            let server = Server::tools(vec![(name.clone(), args)]).await;
            let mut fixture = Fixture::new(remote, &server).await;
            configure(&mut fixture, WorkMode::Plan).await;
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: "Inspect only".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert!(!fixture.root.join("forbidden.txt").exists());
            assert!(page.approvals.is_empty());
            assert!(page.runs[0].status != Status::Running);
            for request in server.requests.lock().unwrap().iter() {
                assert!(!tool_names(request).contains(&name.as_str()));
            }
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
