//! A separate test process owns one sink and only isolated local/remote Nodes.
#[path = "agent_support/mod.rs"]
#[expect(
    dead_code,
    reason = "The shared fixture includes response modes used by other suites"
)]
mod models;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::{Node, logging};
use sailry_protocol::{conversation::*, *};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}
async fn finish(client: &Client, session: SessionId, turn: TurnId) -> Page {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let history = client.read_conversation(session, None, 100).await.unwrap();
            if history.page.runs.iter().any(|run| {
                run.turn == turn
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
            }) {
                return history.page;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
async fn session(client: &Client, endpoint: &str, root: &Path) -> Session {
    let Output::Plugins(packages) = execute(client, Command::ListPlugins).await else {
        panic!("plugins expected")
    };
    for package in packages.into_iter().filter(|package| package.enabled) {
        execute(
            client,
            Command::SetPluginEnabled {
                name: package.name,
                expected_revision: package.revision,
                enabled: false,
            },
        )
        .await;
    }
    let provider = Provider {
        id: ProviderId::new(),
        revision: 0,
        name: "Logging fixture".into(),
        api: ModelApi::ChatCompletions,
        authentication: Authentication::ApiKey,
        endpoint: endpoint.into(),
        enabled: true,
        models: Vec::new(),
        default_model: String::new(),
        credential: None,
        oauth: None,
        options: None,
    };
    let Output::Provider(provider) = execute(
        client,
        Command::PutProvider {
            provider,
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("provider expected")
    };
    let Output::Project(project) = execute(
        client,
        Command::RegisterProject {
            name: "Logging fixture".into(),
            path: root.to_str().unwrap().into(),
        },
    )
    .await
    else {
        panic!("project expected")
    };
    let Output::Session(session) = execute(
        client,
        Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(SessionConfig {
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture-a".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                permission: Permission::Ask,
                credential: None,
            }),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    session
}

#[tokio::test]
async fn observes_shared_lifecycles_without_payloads() {
    let directory = tempfile::tempdir().unwrap();
    let guard = logging::install(directory.path(), logging::Application::Host).unwrap();
    let another = tempfile::tempdir().unwrap();
    assert!(logging::install(another.path(), logging::Application::Desktop).is_err());
    assert_eq!(
        logging::directory(),
        Some(directory.path().join("logs").as_path())
    );
    let file = logging::current_file().unwrap().unwrap();
    assert_eq!(file.parent().unwrap(), directory.path().join("logs/host"));
    assert_eq!(file.extension().unwrap(), "log");
    let mut expected = Vec::new();
    for remote in [false, true] {
        for stop in [false, true] {
            let profile = tempfile::tempdir().unwrap();
            let root = profile.path().join("project");
            std::fs::create_dir(&root).unwrap();
            let started = std::sync::Arc::new(tokio::sync::Notify::new());
            let held = std::sync::Arc::new(tokio::sync::Notify::new());
            let model = if stop {
                models::Server::held(started.clone(), held).await
            } else {
                models::Server::tools(vec![]).await
            };
            let node = Node::start_isolated(profile.path().join("node"))
                .await
                .unwrap();
            assert!(matches!(
                Node::start_isolated(profile.path().join("node")).await,
                Err(sailry_node_runtime::Error::ProfileInUse(_))
            ));
            let controller =
                Link::controller(profile.path().join("controller"), NetworkScope::default())
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
            let session = session(&client, &model.endpoint, &root).await;
            let request = client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "PRIVATE_CONVERSATION_INPUT".into(),
            });
            let admission = client.dispatch(request.clone()).await.unwrap();
            assert!(admission.receipt.durable);
            let Output::QueuedTurn(turn) = admission.completion.await.unwrap().unwrap() else {
                panic!("queued turn expected")
            };
            if stop {
                model.wait_count(1).await;
                execute(&client, Command::StopTurn { turn: turn.id }).await;
            }
            let page = finish(&client, session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                if stop {
                    Status::Cancelled
                } else {
                    Status::Completed
                }
            );
            expected.push((
                serde_json::to_string(&node.id()).unwrap(),
                request.id.to_string(),
                turn.id.to_string(),
                stop,
            ));
            if !stop {
                let package = root.join("package");
                std::fs::create_dir_all(package.join("dev.sailry.platform")).unwrap();
                std::fs::write(package.join("plugin.json"), json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":"diagnostic-fixture", "version":"0.1.0", "extensions":{"dev.sailry.platform":{"api_version":"v1", "actions":[], "host":{"entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"], "handlers":["run"]}}}}).to_string()).unwrap();
                std::fs::write(
                    package.join("dev.sailry.platform/main.js"),
                    "export function run(input) { throw { private: input.private }; }",
                )
                .unwrap();
                let Output::Plugin(package) = execute(
                    &client,
                    Command::InstallPlugin {
                        worktree: session.worktree,
                        path: "package".into(),
                        name: "diagnostic-fixture".into(),
                        expected_revision: 0,
                        expected_digest: None,
                    },
                )
                .await
                else {
                    panic!("package expected")
                };
                let context = plugin::Context {
                    package: package.summary.reference(),
                    worktree: Some(session.worktree),
                    session: Some(session.id),
                    turn: None,
                    invocation: None,
                    surface: Default::default(),
                };
                let fault = client
                    .execute(
                        client
                            .prepare(Command::CallPlugin {
                                handler: "run".into(),
                                input: json!({"private":"PRIVATE_PLUGIN_INPUT"}),
                            })
                            .with_plugin(context),
                    )
                    .await
                    .unwrap_err();
                assert_eq!(fault.code, ErrorCode::InvalidRequest);
            }
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
        }
    }
    assert_eq!(guard.dropped_lines(), 0);
    drop(guard);
    let records = std::fs::read_dir(directory.path().join("logs/host"))
        .unwrap()
        .flat_map(|entry| {
            std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (node, request, turn, stopped) in expected {
        for phase in ["begin", "admitted", "completed"] {
            assert!(records.iter().any(|record| record["area"] == "command"
                && record["request"] == request
                && record["phase"] == phase));
        }
        assert!(records.iter().any(|record| record["area"] == "run"
            && record["node"] == node
            && record["turn"] == turn
            && record["phase"] == if stopped { "cancelled" } else { "completed" }));
        assert!(records.iter().any(|record| record["area"] == "model"
            && record["turn"] == turn
            && record["phase"] == "begin"));
        if !stopped {
            assert!(records.iter().any(|record| record["area"] == "model"
                && record["turn"] == turn
                && record["phase"] == "first_token"
                && record["first_token_us"].is_u64()));
        }
    }
    assert!(records.iter().any(|record| record["area"] == "plugin"
        && record["package"] == "diagnostic-fixture"
        && record["phase"] == "failed"));
    assert!(records.iter().any(|record| record["area"] == "node"
        && record["reason"] == "startup"
        && record["phase"] == "failed"
        && record["code"] == "busy"));
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("PRIVATE_")
    );
}
