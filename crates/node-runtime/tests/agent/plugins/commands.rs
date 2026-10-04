#![cfg(unix)]
use super::*;
use process::Fixture;
use sailry_protocol::{plugin, process::Outcome};
use serde_json::{Value, json};

#[path = "commands/lifecycle.rs"]
mod lifecycle;

fn name(tool: &str) -> String {
    agent_support::plugin_tool("commands", tool)
}

async fn execute(fixture: &Fixture, command: Command) -> Output {
    fixture
        .client
        .execute(fixture.client.prepare(command))
        .await
        .unwrap()
}

async fn package(fixture: &Fixture) -> plugin::Info {
    let Output::Plugin(info) = execute(
        fixture,
        Command::ReadPlugin {
            name: "commands".into(),
        },
    )
    .await
    else {
        panic!("package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    assert!(info.summary.enabled);
    assert_eq!(info.summary.revision, 1);
    info
}

fn scope(fixture: &Fixture, info: &plugin::Info) -> plugin::Context {
    plugin::Context {
        invocation: None,
        turn: None,
        surface: Default::default(),
        package: info.summary.reference(),
        worktree: Some(fixture.session.worktree),
        session: Some(fixture.session.id),
    }
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

#[tokio::test]
async fn reuses_approved_arguments() {
    for remote in [false, true] {
        let command = "printf '记录 🙂' >> once.txt; printf 'stdout'; printf 'stderr' >&2; exit 7";
        let server = Server::tools(vec![
            (name("run_command"), json!({"command":command})),
            (
                name("run_command"),
                json!({"command":"printf before; sleep 60", "timeout_ms":100}),
            ),
            (name("run_command"), json!({"command":"touch denied.txt"})),
        ])
        .await;
        let fixture = Fixture::new(remote, &server).await;
        let info = package(&fixture).await;
        let turn = approvals::submit(&fixture.client, fixture.session.id).await;
        let (_, first) = approvals::pending(&fixture.client, fixture.session.id).await;
        assert!(!fixture.root.join("once.txt").exists());
        process::decide(&fixture.client, &first, Decision::Approve).await;
        let (_, second) = approvals::pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &second, Decision::Approve).await;
        let (_, third) = approvals::pending(&fixture.client, fixture.session.id).await;
        process::decide(&fixture.client, &third, Decision::Deny).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let call = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .find_map(|part| match part {
                Part::ToolCall {
                    display: Some(display),
                    arguments,
                    ..
                } => Some((display, arguments)),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            call.0
                .input
                .as_ref()
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .read(call.1),
            Some(command)
        );
        let output = process::results(&page);
        assert_eq!(output.len(), 2, "{page:?}");
        assert_eq!(output[0].outcome, Outcome::Exited(7));
        assert_eq!(output[0].stdout.text, "stdout");
        assert_eq!(output[0].stderr.text, "stderr");
        assert_eq!(output[1].outcome, Outcome::TimedOut);
        assert_eq!(output[1].stdout.text, "before");
        assert_eq!(results(&page)[1]["isError"], true);
        for value in results(&page).into_iter().take(2) {
            let content = sailry_protocol::tool::Presentation::Content
                .content(value)
                .unwrap();
            assert!(
                content
                    .blocks
                    .iter()
                    .all(|block| matches!(block, sailry_protocol::tool::Block::Text(_)))
            );
        }
        assert!(results(&page)[0].get("isError").is_none());
        assert!(!fixture.root.join("denied.txt").exists());
        let database =
            rusqlite::Connection::open(fixture.node.profile().join("storage/node.sqlite3"))
                .unwrap();
        let id: String = database
            .query_row(
                "SELECT request FROM agent_approvals WHERE id=?1",
                [first.id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        drop(database);
        let request = Request {
            id: id.parse().unwrap(),
            ..fixture
                .client
                .prepare(Command::RunCommand {
                    turn,
                    command: command.into(),
                    cwd: String::new(),
                    timeout_ms: 120000,
                    background: false,
                    attachments: vec![],
                })
                .with_plugin(plugin::Context {
                    turn: Some(turn),
                    ..scope(&fixture, &info)
                })
        };
        let expected = Output::CommandResult(output[0].clone());
        assert_eq!(
            fixture.client.execute(request.clone()).await.unwrap(),
            expected
        );
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("once.txt")).unwrap(),
            "记录 🙂"
        );
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(profile).await.unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, page);
        assert_eq!(client.execute(request).await.unwrap(), expected);
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("once.txt")).unwrap(),
            "记录 🙂"
        );
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_background_processes_and_scopes_public_actions() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (
                name("run_command"),
                json!({"command":"printf ready; sleep 60", "background":true}),
            ),
            (name("read_command"), json!({})),
        ])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        let info = package(&fixture).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let Output::QueuedTurn(turn) = execute(
            &fixture,
            Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Run a background command".into(),
            },
        )
        .await
        else {
            panic!()
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        let result = results(&page);
        assert_eq!(result[0]["kind"], "command_started", "{page:?}");
        let id: RequestId = serde_json::from_value(result[0]["data"]["id"].clone()).unwrap();
        assert_eq!(result[1]["kind"], "commands");
        assert_eq!(result[1]["data"][0]["id"], json!(id));
        let context = scope(&fixture, &info);
        let read = fixture
            .client
            .prepare(Command::ReadCommand {
                session: fixture.session.id,
                id,
            })
            .with_plugin(context.clone());
        let Output::CommandOutput(snapshot) = fixture.client.execute(read.clone()).await.unwrap()
        else {
            panic!()
        };
        assert_eq!(
            snapshot.info.status,
            sailry_protocol::process::Status::Running
        );
        for command in [
            Command::ListCommands {
                session: SessionId::new(),
            },
            Command::ReadCommand {
                session: SessionId::new(),
                id,
            },
            Command::StopCommand {
                session: SessionId::new(),
                id,
            },
        ] {
            assert_eq!(
                fixture
                    .client
                    .execute(fixture.client.prepare(command).with_plugin(context.clone()))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        let mut wrong = context.clone();
        wrong.session = None;
        assert_eq!(
            fixture
                .client
                .execute(Request {
                    plugin: Some(wrong),
                    ..read.clone()
                })
                .await
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
        let stopping = Server::turn_tools(vec![
            (name("stop_command"), json!({"id":id})),
            (name("read_command"), json!({"id":id})),
        ])
        .await;
        let Output::Providers(providers) = execute(&fixture, Command::ListProviders).await else {
            panic!()
        };
        let mut provider = providers
            .into_iter()
            .find(|provider| provider.id == fixture.session.config.provider)
            .unwrap();
        let revision = provider.revision;
        provider.endpoint = stopping.endpoint.clone();
        execute(
            &fixture,
            Command::PutProvider {
                provider,
                expected_revision: revision,
            },
        )
        .await;
        let Output::QueuedTurn(turn) = execute(
            &fixture,
            Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Stop the background command".into(),
            },
        )
        .await
        else {
            panic!()
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        let result = results(&page);
        assert_eq!(result[2]["kind"], "command_output", "{page:?}");
        assert_eq!(result[2]["data"]["info"]["id"], json!(id));
        assert_eq!(result[3]["kind"], "command_output");
        assert_eq!(page.approvals.len(), 2);
        let stop = fixture
            .client
            .prepare(Command::StopCommand {
                session: fixture.session.id,
                id,
            })
            .with_plugin(context.clone());
        let stopped = fixture.client.execute(stop.clone()).await.unwrap();
        assert_eq!(fixture.client.execute(stop).await.unwrap(), stopped);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let Output::CommandOutput(snapshot) =
                    fixture.client.execute(read.clone()).await.unwrap()
                else {
                    panic!()
                };
                if matches!(
                    snapshot.info.status,
                    sailry_protocol::process::Status::Finished(_)
                ) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        execute(
            &fixture,
            Command::SetPluginEnabled {
                name: "commands".into(),
                expected_revision: 1,
                enabled: false,
            },
        )
        .await;
        assert_eq!(
            fixture.client.execute(read).await.unwrap_err().code,
            ErrorCode::NotConfigured
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn plan_mode_excludes_process_writes() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let mut fixture = Fixture::new(remote, &server).await;
        package(&fixture).await;
        planning::configure(&mut fixture, WorkMode::Plan).await;
        let Output::QueuedTurn(turn) = execute(
            &fixture,
            Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Inspect tools".into(),
            },
        )
        .await
        else {
            panic!()
        };
        finished(&fixture.client, fixture.session.id, turn.id).await;
        let request = server.requests.lock().unwrap().last().unwrap().clone();
        let tools = request["tools"].as_array().unwrap();
        assert!(
            tools
                .iter()
                .any(|tool| tool["function"]["name"] == name("read_command"))
        );
        for name in [name("run_command"), name("stop_command")] {
            assert!(!tools.iter().any(|tool| tool["function"]["name"] == name));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_invalid_inputs_with_typed_errors() {
    for remote in [false, true] {
        let server = Server::tools(vec![
            (
                name("run_command"),
                json!({"command":"touch forbidden.txt", "turn":TurnId::new()}),
            ),
            (name("read_command"), json!({"id":"invalid-command-id"})),
        ])
        .await;
        let mut fixture = Fixture::new(remote, &server).await;
        package(&fixture).await;
        planning::configure(&mut fixture, WorkMode::Code).await;
        let Output::QueuedTurn(turn) = execute(
            &fixture,
            Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Check invalid command arguments".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        let page = finished(&fixture.client, fixture.session.id, turn.id).await;
        let output = results(&page);
        assert_eq!(output.len(), 2);
        for result in output {
            assert_eq!(result["error"]["code"], "invalid_request", "{result}");
            assert_eq!(result["isError"], true);
        }
        assert!(!fixture.root.join("forbidden.txt").exists());
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
