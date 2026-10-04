use super::*;
use sailry_protocol::{dispatch as jobs, plugin};
use serde_json::{Value, json};

async fn context(fixture: &process::Fixture) -> plugin::Context {
    let Output::Plugin(package) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadPlugin {
            name: "scheduled-tasks".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("scheduled tasks package expected")
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
        panic!("scheduled task result expected")
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

async fn submit(fixture: &process::Fixture, message: &str) -> TurnId {
    let Output::Session(session) = fixture
        .client
        .execute(fixture.client.prepare(Command::ReadSession {
            session: fixture.session.id,
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    let Output::QueuedTurn(turn) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
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

#[tokio::test]
async fn shares_scope_and_config_with_management() {
    for remote in [false, true] {
        let current = ScheduleId::new();
        let foreign = ScheduleId::new();
        let unassigned = ScheduleId::new();
        let due = chrono::Utc::now().timestamp_millis() + 3_600_000;
        let tool = plugin_tool("scheduled-tasks", "scheduled_tasks");
        let server = Server::tools(vec![
            (tool.clone(), json!({"action":"list"})),
            (tool.clone(), json!({"action":"update","id":current,"revision":"1","name":"Reviewed"})),
            (tool.clone(), json!({"action":"create","name":"Next review","prompt":"Review the project 中文 🙂","timing":{"kind":"every","data":{"anchor_ms":due,"interval_ms":86_400_000}}})),
            (tool.clone(), json!({"action":"delete","id":current,"revision":"2"})),
            (tool.clone(), json!({"action":"list"})),
            (tool.clone(), json!({"action":"update","id":foreign,"revision":"1","name":"Wrong project"})),
            (tool, json!({"action":"delete","id":unassigned,"revision":"1"})),
        ]).await;
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
        for (id, project, name) in [
            (current, fixture.session.project, "Current project"),
            (foreign, Some(project.id), "Other project"),
            (unassigned, None, "Unassigned"),
        ] {
            let saved = call(&fixture.client, &context, "save", json!({
                "id":id,"project":project,"worktree":null,"revision":"0","name":name,
                "prompt":"Retain this prompt","enabled":false,"queue":"default","config":fixture.session.config,
                "timing":{"kind":"once","data":{"at_ms":due}}
            })).await;
            assert_eq!(saved["Ok"]["revision"], "1", "{saved}");
        }
        let original_config = fixture.session.config.clone();
        planning::configure(&mut fixture, WorkMode::Code).await;
        let captured_config = fixture.session.config.clone();
        let Output::QueuedTurn(turn) = fixture
            .client
            .execute(fixture.client.prepare(Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Manage scheduled tasks".into(),
            }))
            .await
            .unwrap()
        else {
            panic!("queued turn expected");
        };
        planning::configure(&mut fixture, WorkMode::Plan).await;
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
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let output = results(&page);
        assert_eq!(output.len(), 7, "{page:?}");
        assert_eq!(output[0]["items"].as_array().unwrap().len(), 1);
        assert_eq!(output[0]["items"][0]["id"], json!(current));
        assert_eq!(output[1]["name"], "Reviewed");
        assert_eq!(output[1]["revision"], "2");
        assert_eq!(output[1]["config"], json!(original_config));
        let created = output[2].clone();
        assert_eq!(created["name"], "Next review");
        assert_eq!(created["project"], json!(fixture.session.project));
        assert_eq!(created["worktree"], json!(fixture.session.worktree));
        assert_eq!(created["config"], json!(captured_config));
        assert_ne!(created["config"], json!(fixture.session.config));
        assert_eq!(created["revision"], "1");
        assert_eq!(created["enabled"], true);
        assert_eq!(output[3]["removed"], true);
        assert_eq!(output[4]["items"].as_array().unwrap().len(), 1);
        assert_eq!(output[4]["items"][0]["id"], created["id"]);
        for result in &output[5..] {
            assert_eq!(result["isError"], true);
            assert_eq!(result["error"]["code"], "permission_denied");
        }
        assert_eq!(page.approvals.len(), 7);
        assert!(
            page.approvals
                .iter()
                .all(|approval| approval.source == ApprovalSource::Full)
        );
        let listed = call(&fixture.client, &context, "list", json!({})).await;
        let items = listed["Ok"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert!(
            items
                .iter()
                .any(|item| item["id"] == created["id"] && item["config"] == created["config"])
        );
        let pending = schedules(&fixture.client, &context).await;
        assert_eq!(pending.len(), 3);
        assert_eq!(
            pending.iter().filter(|schedule| schedule.enabled).count(),
            1
        );
        assert!(
            pending
                .iter()
                .any(|schedule| json!(schedule.id) == created["id"] && schedule.enabled)
        );
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
async fn denied_creation_leaves_no_record_or_schedule() {
    for remote in [false, true] {
        let server = Server::tools(vec![(plugin_tool("scheduled-tasks", "scheduled_tasks"), json!({
            "action":"create","name":"Not authorized","prompt":"Review","timing":{"kind":"once","data":{"at_ms":chrono::Utc::now().timestamp_millis()+3_600_000}}
        }))]).await;
        let fixture = process::Fixture::new(remote, &server).await;
        let context = context(&fixture).await;
        let turn = submit(&fixture, "Create a scheduled task").await;
        let (_, approval) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(schedules(&fixture.client, &context).await.is_empty());
        process::decide(&fixture.client, &approval, Decision::Deny).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(page.approvals.len(), 1);
        assert_eq!(page.approvals[0].state, ApprovalState::Denied);
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

#[tokio::test]
async fn tool_visibility_follows_package_and_planning_policy() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = process::Fixture::new(remote, &server).await;
        let tool = plugin_tool("scheduled-tasks", "scheduled_tasks");
        for (enabled, mode) in [
            (true, WorkMode::Code),
            (false, WorkMode::Code),
            (true, WorkMode::Plan),
        ] {
            let Output::Plugin(package) = fixture
                .client
                .execute(fixture.client.prepare(Command::ReadPlugin {
                    name: "scheduled-tasks".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("package expected")
            };
            if package.summary.enabled != enabled {
                fixture
                    .client
                    .execute(fixture.client.prepare(Command::SetPluginEnabled {
                        name: package.summary.name,
                        expected_revision: package.summary.revision,
                        enabled,
                    }))
                    .await
                    .unwrap();
            }
            planning::configure(&mut fixture, mode).await;
            let turn = submit(&fixture, "Inspect scheduled tools").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
            let requests = server.requests.lock().unwrap();
            let exposed = requests.last().unwrap()["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|declaration| declaration["function"]["name"] == tool);
            assert_eq!(exposed, enabled && mode == WorkMode::Code);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn due_task_runs_after_the_controller_disconnects() {
    for remote in [false, true] {
        let prompt = "Create the scheduled review";
        let idle = Server::tools(vec![]).await;
        let mut fixture = process::Fixture::new(remote, &idle).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let context = context(&fixture).await;
        let due = chrono::Utc::now().timestamp_millis() + 8_000;
        let server = Server::prompt_tools(prompt.into(), vec![(plugin_tool("scheduled-tasks", "scheduled_tasks"), json!({
            "action":"create","name":"Scheduled review","prompt":"Perform the scheduled review 中文 🙂","timing":{"kind":"once","data":{"at_ms":due}}
        }))]).await;
        let Output::Providers(mut providers) = fixture
            .client
            .execute(fixture.client.prepare(Command::ListProviders))
            .await
            .unwrap()
        else {
            panic!("providers expected")
        };
        let mut provider = providers.remove(0);
        provider.endpoint = server.endpoint.clone();
        fixture
            .client
            .execute(fixture.client.prepare(Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            }))
            .await
            .unwrap();
        let turn = submit(&fixture, prompt).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let output = results(&page);
        assert_eq!(output.len(), 1, "{page:?}");
        assert_eq!(output[0]["name"], "Scheduled review", "{output:?}");
        let id = output[0]["id"].clone();
        let config = fixture.session.config.clone();
        let project = fixture.session.project;
        fixture.controller.close().await.unwrap();
        let client = Client::new(fixture.node.local());
        let execution = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let listed = call(&client, &context, "history", json!({})).await;
                if let Some(item) = listed["Ok"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| {
                        item["job"]["handler"] == id && item["job"]["status"] == "completed"
                    })
                {
                    break item.clone();
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("scheduled execution deadline");
        let session: SessionId = serde_json::from_value(execution["session"].clone()).unwrap();
        assert_ne!(session, fixture.session.id);
        let Output::Session(scheduled) = client
            .execute(client.prepare(Command::ReadSession { session }))
            .await
            .unwrap()
        else {
            panic!("scheduled session expected")
        };
        assert_eq!(scheduled.project, project);
        assert_eq!(scheduled.config, config);
        let history = history(&client, session).await;
        assert!(
            history
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        );
        assert!(results(&history).is_empty());
        assert_eq!(history.runs.len(), 1);
        assert_eq!(
            text(&history),
            "Perform the scheduled review 中文 🙂answer-fixture-a"
        );
        assert_eq!(
            call(&client, &context, "history", json!({})).await["Ok"]["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(idle.requests.lock().unwrap().is_empty());
        fixture.node.shutdown().await.unwrap();
    }
}
