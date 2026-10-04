use super::*;
use sailry_protocol::{dispatch as jobs, plugin};
use serde_json::{Value, json};

async fn context(fixture: &process::Fixture) -> plugin::Context {
    let Output::Plugin(package) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadPlugin {
            name: "reminders".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("reminders package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
    assert_eq!(package.extension.as_ref().unwrap().tools.len(), 1);
    plugin::Context {
        invocation: None,
        turn: None,
        surface: plugin::desktop::Surface::Workspace,
        package: package.summary.reference(),
        worktree: None,
        session: None,
    }
}

async fn call(client: &Client, context: &plugin::Context, handler: &str, input: Value) -> Value {
    let Output::PluginResult(result) = client
        .execute(
            client
                .prepare(Command::CallPlugin {
                    handler: handler.into(),
                    input,
                })
                .with_plugin(context.clone()),
        )
        .await
        .unwrap()
    else {
        panic!("reminder result expected")
    };
    result
}

async fn schedules(client: &Client, context: &plugin::Context) -> Vec<jobs::Schedule> {
    let Output::Dispatch(jobs::Output::Schedules(schedules)) = client
        .execute(
            client
                .prepare(Command::Dispatch {
                    package: context.package.clone(),
                    action: jobs::Command::ListSchedules,
                })
                .with_plugin(context.clone()),
        )
        .await
        .unwrap()
    else {
        panic!("schedules expected")
    };
    schedules
}

fn results(page: &Page) -> Vec<&Value> {
    page.entries
        .iter()
        .flat_map(|entry| &entry.parts)
        .filter_map(|part| match part {
            Part::ToolResult { result, .. } => Some(result),
            _ => None,
        })
        .collect()
}

async fn submit(fixture: &process::Fixture) -> TurnId {
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: "Manage project reminders".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    turn.id
}

#[tokio::test]
async fn shares_project_records_with_management() {
    for remote in [false, true] {
        let current = ScheduleId::new();
        let other = ScheduleId::new();
        let unassigned = ScheduleId::new();
        let due = chrono::Utc::now().timestamp_millis() + 3_600_000;
        let tool = plugin_tool("reminders", "reminders");
        let server = Server::tools(vec![
            (tool.clone(), json!({"action":"list"})),
            (tool.clone(), json!({"action":"update","id":current,"revision":"1","title":"Reviewed","completed":true})),
            (tool.clone(), json!({"action":"create","title":"Next review","message":"Saved by the conversation","due_ms":due})),
            (tool.clone(), json!({"action":"delete","id":current,"revision":"2"})),
            (tool.clone(), json!({"action":"list"})),
            (tool.clone(), json!({"action":"update","id":other,"revision":"1","title":"Wrong project"})),
            (tool, json!({"action":"delete","id":unassigned,"revision":"1"})),
        ])
        .await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let context = context(&fixture).await;
        let other_root = fixture.root.join("other");
        std::fs::create_dir(&other_root).unwrap();
        let Output::Project(project) = fixture
            .client
            .execute(fixture.client.prepare(Command::RegisterProject {
                name: "Other project".into(),
                path: other_root.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        for (id, project, title) in [
            (current, fixture.session.project, "Current project"),
            (other, Some(project.id), "Other project"),
            (unassigned, None, "No project"),
        ] {
            let result = call(
                &fixture.client,
                &context,
                "save",
                json!({
                    "id":id,"project":project,"revision":"0","title":title,
                    "message":"","completed":false,"due_ms":null
                }),
            )
            .await;
            assert_eq!(result["Ok"]["revision"], "1", "{result}");
        }
        planning::configure(&mut fixture, WorkMode::Code).await;
        let turn = submit(&fixture).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let output = results(&page);
        assert_eq!(output.len(), 7, "{page:?}");
        assert_eq!(output[0]["items"].as_array().unwrap().len(), 1);
        assert_eq!(output[0]["items"][0]["id"], json!(current));
        assert_eq!(output[1]["title"], "Reviewed");
        assert_eq!(output[1]["completed"], true);
        assert_eq!(output[1]["revision"], "2");
        let created = output[2].clone();
        assert_eq!(created["title"], "Next review");
        assert_eq!(created["project"], json!(fixture.session.project));
        assert_eq!(created["revision"], "1");
        assert_ne!(created["id"], json!(turn));
        assert_eq!(output[3]["removed"], true);
        assert_eq!(output[4]["items"], json!([created.clone()]));
        for result in &output[5..] {
            assert_eq!(result["isError"], true);
            assert_eq!(result["error"]["code"], "permission_denied");
        }
        // One multi-action declaration uses the normal ADK confirmation policy.
        assert_eq!(page.approvals.len(), 7);
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.source == ApprovalSource::Full)
        );
        let listed = call(&fixture.client, &context, "list", json!({})).await;
        let items = listed["Ok"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert!(items.contains(&created));
        for (id, title) in [(other, "Other project"), (unassigned, "No project")] {
            let item = items.iter().find(|item| item["id"] == json!(id)).unwrap();
            assert_eq!(item["title"], title);
            assert_eq!(item["revision"], "1");
        }
        let pending = schedules(&fixture.client, &context).await;
        assert_eq!(pending.len(), 3);
        let enabled = pending
            .iter()
            .filter(|schedule| schedule.enabled)
            .collect::<Vec<_>>();
        assert_eq!(enabled.len(), 1);
        assert_eq!(json!(enabled[0].id), created["id"]);
        for id in [other, unassigned] {
            assert!(
                !pending
                    .iter()
                    .find(|schedule| schedule.id == id)
                    .unwrap()
                    .enabled
            );
        }
        assert!(pending.iter().all(|schedule| schedule.id != current));
        let requests = server.requests.lock().unwrap().len();
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, page);
        assert_eq!(call(&client, &context, "list", json!({})).await, listed);
        assert_eq!(schedules(&client, &context).await, pending);
        assert_eq!(server.requests.lock().unwrap().len(), requests);
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn denied_mutation_leaves_no_record_or_schedule() {
    for remote in [false, true] {
        let server = Server::tools(vec![(plugin_tool("reminders", "reminders"), json!({
            "action":"create","title":"Not authorized","due_ms":chrono::Utc::now().timestamp_millis()+3_600_000
        }))])
        .await;
        let fixture = process::Fixture::new(remote, &server).await;
        let context = context(&fixture).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(
            call(&fixture.client, &context, "list", json!({})).await["Ok"]["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(schedules(&fixture.client, &context).await.is_empty());
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].state, ApprovalState::Denied);
        assert_eq!(results(&page).len(), 1);
        assert_eq!(
            results(&page)[0]["error"],
            format!(
                "Tool '{}' execution denied by confirmation policy",
                plugin_tool("reminders", "reminders")
            )
        );
        assert!(
            call(&fixture.client, &context, "list", json!({})).await["Ok"]["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(schedules(&fixture.client, &context).await.is_empty());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
