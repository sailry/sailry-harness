//! A separate test process owns one sink and only isolated local/remote Nodes.
#[path = "agent_support/mod.rs"]
#[expect(
    dead_code,
    reason = "The shared fixture includes response modes used by other suites"
)]
mod models;
#[path = "logging/parse.rs"]
mod parse;
use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::{Node, logging};
use sailry_protocol::{conversation::*, *};
use serde_json::json;
use std::{path::Path, time::Duration};

async fn execute(client: &Client, command: Command) -> Output {
    client.execute(client.prepare(command)).await.unwrap()
}
async fn finish(client: &Client, session: SessionId, turn: TurnId) -> Page {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut updates = client.subscribe_conversation(session).await.unwrap();
        loop {
            let terminal = match updates.next().await.unwrap() {
                Update::ConversationSnapshot(snapshot) => snapshot.page.runs.iter().any(|run| {
                    run.turn == turn && !matches!(run.status, Status::Queued | Status::Running | Status::Stopping)
                }),
                Update::ConversationFrame(frame) => matches!(frame.change,
                    Change::Run(run) if run.turn == turn && !matches!(run.status, Status::Queued | Status::Running | Status::Stopping)),
                Update::ResetRequired => {
                    updates = client.subscribe_conversation(session).await.unwrap();
                    false
                }
                _ => false,
            };
            if terminal {
                return client.read_conversation(session, None, 100).await.unwrap().page;
            }
        }
    })
    .await
    .unwrap()
}
async fn session(client: &Client, endpoint: &str, root: &Path) -> Session {
    session_api(client, endpoint, root, ModelApi::ChatCompletions, None).await
}
async fn session_api(
    client: &Client,
    endpoint: &str,
    root: &Path,
    api: ModelApi,
    retries: Option<u64>,
) -> Session {
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
        request_max_retries: None,
        stream_max_retries: retries,
        preset: None,
        binding: None,
        consent: None,
        id: ProviderId::new(),
        revision: 0,
        name: "Logging fixture".into(),
        api,
        authentication: Authentication::ApiKey,
        endpoint: endpoint.into(),
        enabled: true,
        models: vec![Model {
            id: "fixture-a".into(),
            display_name: None,
            context: 16_000,
            output: 2_048,
            vision: false,
            tools: true,
            reasoning: false,
            web_search: false,
            generates: Vec::new(),
            efforts: Vec::new(),
            capabilities: None,
            custom_efforts: false,
            default_effort: Effort::Default,
        }],
        default_model: "fixture-a".into(),
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
                features: Default::default(),
                assistant: None,
                resource: None,
                provider: provider.id,
                model: "fixture-a".into(),
                effort: Effort::Default,
                mode: WorkMode::Code,
                options: Default::default(),
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
    let profile = directory.path().join("profile");
    assert!(!profile.exists());
    let guard = logging::install(&profile, logging::Application::Host).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            profile.metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let legacy = profile.join("logs/host/2026-10-23.log");
    std::fs::write(&legacy, "old diagnostic file").unwrap();
    let node = Node::start_empty(profile.clone()).await.unwrap();
    assert!(!legacy.exists());
    std::fs::write(&legacy, "active diagnostic file").unwrap();
    assert!(matches!(
        Node::start_empty(profile.clone()).await,
        Err(sailry_node_runtime::Error::ProfileInUse(_))
    ));
    assert!(legacy.exists());
    node.shutdown().await.unwrap();
    let node = Node::start_empty(profile.clone()).await.unwrap();
    assert!(!legacy.exists());
    node.shutdown().await.unwrap();
    let another = tempfile::tempdir().unwrap();
    assert!(
        logging::install(
            &another.path().join("profile"),
            logging::Application::Desktop
        )
        .is_err()
    );
    assert_eq!(logging::directory(), Some(profile.join("logs").as_path()));
    let file = logging::current_file().unwrap().unwrap();
    assert_eq!(file.parent().unwrap(), profile.join("logs/host"));
    assert_eq!(file.extension().unwrap(), "log");
    let mut expected = Vec::new();
    let mut summaries = Vec::new();
    // Reopen the same isolated profiles so each scenario retains real Node
    // startup/shutdown logging without reinstalling the same bundled packages.
    let profiles = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    for remote in [false, true] {
        for stop in [false, true] {
            let profile = &profiles[usize::from(remote)];
            let root = profile.path().join(format!("project-{stop}"));
            std::fs::create_dir_all(&root).unwrap();
            let started = std::sync::Arc::new(tokio::sync::Notify::new());
            let held = std::sync::Arc::new(tokio::sync::Notify::new());
            let model = if stop {
                models::Server::held(started.clone(), held).await
            } else {
                models::Server::tools(vec![]).await
            };
            let node = Node::start_empty(profile.path().join("node"))
                .await
                .unwrap();
            assert!(matches!(
                Node::start_empty(profile.path().join("node")).await,
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
                node.id()
                    .0
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
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
    let retry_profile = &profiles[1];
    let retry_root = retry_profile.path().join("retry-project");
    std::fs::create_dir(&retry_root).unwrap();
    let retry_model = models::Server::http(1, 500).await;
    let retry_node = Node::start_empty(retry_profile.path().join("node"))
        .await
        .unwrap();
    retry_node.set_model_retry_fixture(Duration::ZERO);
    let retry_controller = Link::controller(
        retry_profile.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let retry_address = retry_controller
        .handle()
        .pair(retry_node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let retry_client = Client::new(retry_controller.handle().remote(retry_address));
    let retry_session = session(&retry_client, &retry_model.endpoint, &retry_root).await;
    let Output::QueuedTurn(retry_turn) = execute(
        &retry_client,
        Command::SubmitTurn {
            session: retry_session.id,
            expected_revision: retry_session.revision,
            message: "Retry once".into(),
        },
    )
    .await
    else {
        panic!("queued turn expected")
    };
    let retry_page = finish(&retry_client, retry_session.id, retry_turn.id).await;
    assert_eq!(
        retry_page.runs.last().unwrap().status,
        Status::Completed,
        "{:?}",
        retry_page.runs.last().unwrap().error
    );
    assert_eq!(retry_model.requests.lock().unwrap().len(), 2);
    let rate_model = models::Server::http(usize::MAX, 429).await;
    let rate_root = retry_profile.path().join("rate-project");
    std::fs::create_dir(&rate_root).unwrap();
    let rate_session = session(&retry_client, &rate_model.endpoint, &rate_root).await;
    let Output::QueuedTurn(rate_turn) = execute(
        &retry_client,
        Command::SubmitTurn {
            session: rate_session.id,
            expected_revision: rate_session.revision,
            message: "Keep throttling visible".into(),
        },
    )
    .await
    else {
        panic!("queued turn expected")
    };
    let rate_page = finish(&retry_client, rate_session.id, rate_turn.id).await;
    let rate_run = rate_page.runs.last().unwrap();
    assert_eq!(rate_run.status, Status::Failed);
    let error = rate_run.error.as_ref().unwrap();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(error.message.contains("HTTP 429") && error.message.contains("fixture unavailable"));
    assert!(matches!(
        error.details.as_ref(),
        Some(FaultDetails::ModelRecovery {
            reason: recovery::Stop::AttemptsExhausted
        })
    ));
    assert_eq!(rate_model.requests.lock().unwrap().len(), 6);
    retry_node.shutdown().await.unwrap();
    retry_controller.close().await.unwrap();

    let parse_profile = &profiles[1];
    let parse_root = parse_profile.path().join("parse-project");
    std::fs::create_dir(&parse_root).unwrap();
    let parse_model = models::Server::responses_parse().await;
    let parse_node = Node::start_empty(parse_profile.path().join("node"))
        .await
        .unwrap();
    parse_node.set_model_transport_fixture(adk_model::openai::WsOptions {
        enabled: std::sync::Arc::new(|| false),
        ..Default::default()
    });
    let parse_controller = Link::controller(
        parse_profile.path().join("controller"),
        NetworkScope::default(),
    )
    .await
    .unwrap();
    let parse_address = parse_controller
        .handle()
        .pair(parse_node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    let parse_client = Client::new(parse_controller.handle().remote(parse_address));
    let parse_session = session_api(
        &parse_client,
        &parse_model.endpoint,
        &parse_root,
        ModelApi::Responses,
        // This scenario observes one parse failure; recovery has separate tests.
        Some(0),
    )
    .await;
    let Output::QueuedTurn(parse_turn) = execute(
        &parse_client,
        Command::SubmitTurn {
            session: parse_session.id,
            expected_revision: parse_session.revision,
            message: "PRIVATE_PARSE_INPUT".into(),
        },
    )
    .await
    else {
        panic!("queued turn expected")
    };
    let parse_page = finish(&parse_client, parse_session.id, parse_turn.id).await;
    assert_eq!(parse_page.runs.last().unwrap().status, Status::Failed);
    assert_eq!(parse_model.requests.lock().unwrap().len(), 1);
    parse_node.shutdown().await.unwrap();
    parse_controller.close().await.unwrap();

    for (remote, mode, usage) in [(false, "manual", 100), (true, "automatic", 14_000)] {
        let profile = &profiles[usize::from(remote)];
        let root = profile.path().join(format!("summary-{mode}"));
        std::fs::create_dir(&root).unwrap();
        let model = models::Server::summary(usage).await;
        let node = Node::start_empty(profile.path().join("node"))
            .await
            .unwrap();
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
        let turns = if mode == "automatic" { 4 } else { 1 };
        for index in 0..turns {
            let Output::QueuedTurn(turn) = execute(
                &client,
                Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: format!("Summary evidence {index}").into(),
                },
            )
            .await
            else {
                panic!("queued turn expected")
            };
            let page = finish(&client, session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        }
        if mode == "manual" {
            let Output::QueuedTurn(turn) = execute(
                &client,
                Command::CompactContext {
                    session: session.id,
                    expected_revision: session.revision,
                },
            )
            .await
            else {
                panic!("compaction turn expected")
            };
            let page = finish(&client, session.id, turn.id).await;
            assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        }
        assert!(
            model
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(models::compaction::is_summary)
        );
        summaries.push((session.id.to_string(), mode));
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
    assert_eq!(guard.dropped_lines(), 0);
    drop(guard);
    let records = std::fs::read_dir(profile.join("logs/host"))
        .unwrap()
        .flat_map(|entry| {
            std::fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .lines()
                .map(parse::line)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (node, request, turn, stopped) in expected {
        for phase in ["begin", "admitted", "completed"] {
            assert!(records.iter().any(|record| record["area"] == "command"
                && record["request"] == request
                && record["phase"] == phase));
        }
        assert!(records.iter().any(|record| record["area"] == "command"
            && record["request"] == request
            && record["turn"] == turn
            && record["phase"] == "completed"));
        assert!(records.iter().any(|record| record["area"] == "run"
            && record["node"] == node
            && record["turn"] == turn
            && record["phase"] == if stopped { "cancelled" } else { "completed" }));
        let model: Vec<_> = records
            .iter()
            .filter(|record| {
                record["area"] == "model"
                    && record["turn"] == turn
                    && record["operation"] == "model_request"
            })
            .collect();
        assert_eq!(model.len(), 2);
        assert!(model.iter().any(|record| record["phase"] == "begin"
            && record["stream"] == true
            && record["budget"] == 2048
            && record.get("request_mode").is_none()));
        if !stopped {
            assert!(model.iter().any(|record| record["area"] == "model"
                && record["phase"] == "completed"
                && record["first_token_us"].is_u64()));
            assert!(
                !records
                    .iter()
                    .any(|record| record["turn"] == turn && record["operation"] == "model_retry")
            );
        } else {
            let terminal = model
                .iter()
                .find(|record| record["phase"] == "cancelled")
                .unwrap();
            assert_eq!(terminal["code"], "cancelled");
            assert!(terminal.get("cause_code").is_none());
        }
    }
    assert!(
        !records
            .iter()
            .any(|record| record["phase"] == "first_token")
    );
    let recoveries: Vec<_> = records
        .iter()
        .filter(|record| {
            record["turn"] == retry_turn.id.to_string() && record["operation"] == "model_retry"
        })
        .collect();
    assert_eq!(recoveries.len(), 1);
    assert_eq!(recoveries[0]["attempt"], 1);
    assert_eq!(recoveries[0]["limit"], 5);
    assert_eq!(recoveries[0]["wait_ms"], 60_000);
    let attempts: Vec<_> = records
        .iter()
        .filter(|record| {
            record["turn"] == retry_turn.id.to_string() && record["operation"] == "model_attempt"
        })
        .collect();
    assert_eq!(attempts.len(), 4);
    let retry = attempts
        .iter()
        .find(|record| record["phase"] == "failed")
        .unwrap();
    assert_eq!(retry["phase"], "failed");
    assert_eq!(retry["attempt"], 1);
    assert_eq!(retry["cause_code"], "model.http.status");
    assert_eq!(retry["cause_component"], "model");
    assert_eq!(retry["cause_category"], "unavailable");
    assert_eq!(retry["upstream_status"], 500);
    assert!(attempts.iter().any(|record| record["phase"] == "completed"
        && record["attempt"] == 2
        && record["upstream_status"] == 200));
    let failures = records
        .iter()
        .filter(|record| {
            record["turn"] == rate_turn.id.to_string()
                && record["operation"] == "model_request"
                && record["phase"] == "failed"
        })
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["cause_code"], "model.openai_compat.api_error");
    assert_eq!(failures[0]["cause_category"], "rate_limited");
    assert_eq!(failures[0]["upstream_status"], 429);
    let attempts: Vec<_> = records
        .iter()
        .filter(|record| {
            record["turn"] == rate_turn.id.to_string()
                && record["operation"] == "model_attempt"
                && record["phase"] == "failed"
        })
        .collect();
    assert_eq!(attempts.len(), 6);
    for (index, record) in attempts.iter().enumerate() {
        assert_eq!(record["attempt"], index + 1);
        assert_eq!(record["upstream_status"], 429);
    }
    let recoveries: Vec<_> = records
        .iter()
        .filter(|record| {
            record["turn"] == rate_turn.id.to_string() && record["operation"] == "model_retry"
        })
        .collect();
    assert_eq!(recoveries.len(), 5);
    for (index, record) in recoveries.iter().enumerate() {
        assert_eq!(record["attempt"], index + 1);
        assert_eq!(record["limit"], 5);
        assert_eq!(record["wait_ms"], 60_000);
    }
    let parse: Vec<_> = records
        .iter()
        .filter(|record| {
            record["turn"] == parse_turn.id.to_string() && record["operation"] == "model_request"
        })
        .collect();
    assert_eq!(parse.len(), 2);
    let failure = parse
        .iter()
        .find(|record| record["phase"] == "failed")
        .unwrap();
    assert_eq!(failure["cause_code"], "model.openai_responses.parse");
    assert_eq!(failure["cause_component"], "model");
    assert_eq!(failure["cause_category"], "internal");
    assert_eq!(failure["parse_layer"], "stream_event");
    assert_eq!(failure["parse_category"], "data");
    assert_eq!(failure["parse_cause"], "missing_field");
    assert_eq!(failure["parse_field"], "output_index");
    assert_eq!(failure["parse_event"], "response.output_text.delta");
    assert!(failure["parse_line"].is_u64());
    assert!(failure["parse_column"].is_u64());
    let attempts = records
        .iter()
        .filter(|record| {
            record["turn"] == parse_turn.id.to_string() && record["operation"] == "model_attempt"
        })
        .collect::<Vec<_>>();
    assert!(!attempts.is_empty());
    assert!(attempts.iter().all(|record| record["transport"] == "http"));
    assert!(!records.iter().any(|record| {
        record["turn"] == parse_turn.id.to_string() && record["operation"] == "model_retry"
    }));
    for (session, mode) in summaries {
        let summary: Vec<_> = records
            .iter()
            .filter(|record| {
                record["session"] == session
                    && record["operation"] == "summary_request"
                    && record["source_kind"] == mode
            })
            .collect();
        let begun = summary
            .iter()
            .filter(|record| record["phase"] == "begin")
            .count();
        let completed: Vec<_> = summary
            .iter()
            .filter(|record| record["phase"] == "completed")
            .collect();
        assert!(begun > 0);
        assert_eq!(begun, completed.len());
        assert!(
            completed
                .iter()
                .all(|record| record["stage"] == "prepared" && record["duration_us"].is_u64())
        );
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
