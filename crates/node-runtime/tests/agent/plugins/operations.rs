use super::*;
use sailry_protocol::{plugin, tool::Presentation};
use serde_json::{Value, json};

fn name(tool: &str) -> String {
    let digest = blake3::hash(b"task-notes").to_hex();
    format!("plugin_{}_{tool}", &digest[..16])
}

async fn execute(fixture: &process::Fixture, command: Command) -> Output {
    fixture
        .client
        .execute(fixture.client.prepare(command))
        .await
        .unwrap()
}

async fn install(fixture: &mut process::Fixture) -> plugin::Info {
    std::fs::create_dir(fixture.root.join("package")).unwrap();
    std::fs::create_dir_all(fixture.root.join("package/dev.sailry.platform/desktop")).unwrap();
    std::fs::write(
        fixture
            .root
            .join("package/dev.sailry.platform/desktop/chat.js"),
        include_str!(
            "../../../../../plugins/examples/task-notes/dev.sailry.platform/desktop/chat.js"
        ),
    )
    .unwrap();
    std::fs::write(
        fixture
            .root
            .join("package/dev.sailry.platform/desktop/main.js"),
        include_str!(
            "../../../../../plugins/examples/task-notes/dev.sailry.platform/desktop/main.js"
        ),
    )
    .unwrap();
    std::fs::write(
        fixture
            .root
            .join("package/dev.sailry.platform/desktop/locales.js"),
        include_str!(
            "../../../../../plugins/examples/task-notes/dev.sailry.platform/desktop/locales.js"
        ),
    )
    .unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../plugins/examples/task-notes/plugin.json"
    ))
    .unwrap();
    for tool in manifest["extensions"]["dev.sailry.platform"]["tools"]
        .as_array_mut()
        .unwrap()
    {
        if tool["name"] == "read_note" {
            tool["grouping"] = json!("standalone");
        }
    }
    std::fs::write(
        fixture.root.join("package/plugin.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let Output::Plugin(info) = execute(
        fixture,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "package".into(),
            name: "task-notes".into(),
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("plugin expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    assert!(info.mcp.is_empty());
    info
}

async fn submit(fixture: &process::Fixture, queued: bool) -> QueuedTurn {
    let command = if queued {
        Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Record the task".into(),
        }
    } else {
        Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Record the task".into(),
        }
    };
    let Output::QueuedTurn(turn) = execute(fixture, command).await else {
        panic!("turn expected")
    };
    turn
}

#[tokio::test]
async fn keeps_admitted_operations_and_history() {
    for remote in [false, true] {
        let progress =
            json!({"title": "Review", "steps":[{"description":"Verify", "state":"completed"}]});
        let server = Server::tools(vec![
            (
                name("save_note"),
                json!({"key":"review", "value":{"done":true}, "expected_revision":0}),
            ),
            (name("read_note"), json!({"key":"review"})),
            (name("progress"), progress.clone()),
        ])
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let info = install(&mut fixture).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let turn = submit(&fixture, true).await;
        let path = fixture.root.join("package/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        manifest["version"] = json!("2.0.0");
        for tool in manifest["extensions"]["dev.sailry.platform"]["tools"]
            .as_array_mut()
            .unwrap()
        {
            if tool["name"] == "save_note" {
                tool["operation"] = json!("storage.get");
            }
        }
        std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        execute(
            &fixture,
            Command::InstallPlugin {
                worktree: fixture.session.worktree,
                path: "package".into(),
                name: "task-notes".into(),
                expected_revision: info.summary.revision,
            },
        )
        .await;
        execute(&fixture, Command::StartQueuedTurn { turn: turn.id }).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(
            page.approvals[0].source,
            sailry_protocol::conversation::ApprovalSource::Storage
        );
        let results: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| {
                if let Part::ToolResult { result, .. } = part {
                    Some(result)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["revision"], 1, "{results:?}");
        assert_eq!(results[1]["value"], json!({"done":true}));
        assert_eq!(results[2]["progress"], progress);
        let view = super::super::progress::observe(&fixture.client, fixture.session.id).await;
        assert_eq!(
            view.calls[0].grouping,
            sailry_protocol::tool::Grouping::Sequence
        );
        assert_eq!(
            view.calls[1].grouping,
            sailry_protocol::tool::Grouping::Standalone
        );
        assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| matches!(part, Part::ToolCall { name: tool, grouping: sailry_protocol::tool::Grouping::Standalone, .. } if tool == &name("read_note"))));
        assert_eq!(
            serde_json::to_value(view.calls.last().unwrap().progress.as_ref().unwrap()).unwrap(),
            progress
        );
        assert!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| matches!(
                    part,
                    Part::ToolCall {
                        presentation: Presentation::Progress,
                        ..
                    }
                ))
        );
        let original = page.clone();
        let expired = fixture
            .client
            .prepare(Command::ReadPluginValue {
                key: "review".into(),
            })
            .with_plugin(plugin::Context {
                invocation: None,
                turn: Some(turn.id),
                surface: Default::default(),
                package: info.summary.reference(),
                worktree: Some(fixture.session.worktree),
                session: Some(fixture.session.id),
            });
        assert_eq!(
            fixture.client.execute(expired).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        let path = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(path).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, original);
        assert_eq!(server.requests.lock().unwrap().len(), 4);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn uses_storage_authority_and_filters_plan_writes() {
    for remote in [false, true] {
        let server = Server::tools(vec![(
            name("save_note"),
            json!({"key":"review", "value":"approved", "expected_revision":0}),
        )])
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        install(&mut fixture).await;
        let turn = submit(&fixture, false).await;
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(
            page.approvals[0].state,
            sailry_protocol::conversation::ApprovalState::Approved
        );
        assert_eq!(
            page.approvals[0].source,
            sailry_protocol::conversation::ApprovalSource::Storage
        );
        planning::configure(&mut fixture, WorkMode::Plan).await;
        let turn = submit(&fixture, false).await;
        finished(&fixture.client, fixture.session.id, turn.id).await;
        let request = server.requests.lock().unwrap().last().cloned().unwrap();
        let names: Vec<_> = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&name("read_note").as_str()));
        assert!(names.contains(&name("progress").as_str()));
        assert!(!names.contains(&name("save_note").as_str()));
        assert!(!names.contains(&name("delete_note").as_str()));
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
