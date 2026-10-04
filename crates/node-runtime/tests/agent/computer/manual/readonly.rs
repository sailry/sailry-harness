//! Opt-in real-model observation through the local and paired Node boundaries.
use super::*;
use serde_json::Value;
use std::path::Path;

const MODEL: &str = "gpt-6-luna";
const OBSERVATIONS: [&str; 3] = ["list_apps", "get_accessibility_tree", "get_window_state"];

fn state(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("owned fixture state"))
        .expect("valid owned fixture state")
}

async fn complete(client: &Client, relay: &relay::Relay, session: SessionId, turn: TurnId) -> Page {
    let completed = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let page = history(client, session).await;
            if page.runs.iter().any(|run| {
                run.turn == turn
                    && !matches!(
                        run.status,
                        Status::Queued | Status::Running | Status::Stopping
                    )
            }) {
                return Some(page);
            }
            if relay.refused() {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    if let Ok(Some(page)) = completed {
        return page;
    }
    client
        .execute(client.prepare(Command::StopTurn { turn }))
        .await
        .expect("stop read-only acceptance turn");
    finished(client, session, turn).await
}

#[tokio::test]
#[ignore = "billable gpt-6-luna high acceptance; requires selected API-key provider, inactive source profile, signed worker and owned AppKit fixture state"]
async fn observes_owned_window() {
    assert!(
        std::env::var_os("SAILRY_COMPUTER_BUDGET").is_none(),
        "read-only acceptance cannot resume"
    );
    let fixture =
        std::env::var_os("SAILRY_CUA_FIXTURE_OUTPUT").expect("owned fixture state required");
    let initial = state(Path::new(&fixture));
    let pid = initial["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .unwrap();
    let window = initial["window_id"]
        .as_u64()
        .and_then(|window| u32::try_from(window).ok())
        .unwrap();
    let expected = initial["text"].as_str().expect("owned fixture text");
    assert!(
        pid > 0 && window > 0 && !expected.is_empty(),
        "owned window required"
    );
    for field in ["clicks", "pointer_clicks", "unsupported_presses"] {
        assert!(initial[field].is_u64(), "owned fixture counter required");
    }
    let provider_id = std::env::var("SAILRY_COMPUTER_PROVIDER")
        .expect("explicit provider selection required")
        .parse()
        .unwrap();
    let (provider, secret) = provider_fixture::load_selected(MODEL, provider_id);
    assert_eq!(
        provider.api,
        ModelApi::Responses,
        "Responses acceptance required"
    );
    let relay = relay::Relay::start(
        &provider.endpoint,
        secret,
        scope::Scope::readonly(pid, window, MODEL.into()),
        None,
    )
    .await;
    let directory = tempfile::tempdir().unwrap();
    let mut pages = Vec::new();
    for remote in [false, true] {
        let path = directory
            .path()
            .join(if remote { "remote" } else { "local" });
        let root = path.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let node = Node::start_with_computer(
            path.join("node"),
            NetworkScope::default(),
            Some(sailry_node_runtime::ComputerWorker {
                executable: std::env::var_os("SAILRY_TEST_COMPUTER_WORKER")
                    .expect("signed computer worker required")
                    .into(),
                bundle_id: "ai.sailry.host".into(),
            }),
        )
        .await
        .unwrap();
        let controller = Link::controller(path.join("controller"), NetworkScope::default())
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
        let Output::Plugins(packages) = client
            .execute(client.prepare(Command::ListPlugins))
            .await
            .unwrap()
        else {
            panic!("plugin inventory expected")
        };
        let names: Vec<_> = packages
            .iter()
            .filter(|package| package.enabled && package.name != "computer")
            .map(|package| package.name.as_str())
            .collect();
        disable_tools(&client, &names).await;
        set_enabled(&client, true).await;
        let (session, mut selected) = configured(&client, &relay.endpoint, &root).await;
        selected.api = provider.api;
        selected.authentication = provider.authentication;
        selected.options = provider.options.clone();
        selected.models = provider
            .models
            .iter()
            .filter(|model| model.id == MODEL)
            .cloned()
            .collect();
        selected.default_model = MODEL.into();
        let Output::Provider(selected) = client
            .execute(client.prepare(Command::SaveProvider {
                expected_revision: selected.revision,
                provider: selected,
                secret: Some(sailry_protocol::Secret::new(relay::LOCAL_KEY.into())),
            }))
            .await
            .unwrap()
        else {
            panic!("selected provider expected")
        };
        let mut config = session.config;
        config.provider = selected.id;
        config.credential = selected.credential;
        config.model = MODEL.into();
        config.effort = Effort::High;
        config.mode = WorkMode::Plan;
        config.permission = Permission::Ask;
        let Output::Session(session) = client
            .execute(client.prepare(Command::SetSessionConfig {
                session: session.id,
                expected_revision: session.revision,
                config,
            }))
            .await
            .unwrap()
        else {
            panic!("planning session expected")
        };
        let message = format!(
            "Use list_apps and get_accessibility_tree to discover the available apps and windows. Then use get_window_state to read the accessibility tree of my test window, PID {pid}, window ID {window}, with include_accessibility_tree true and include_screenshot false. Tell me whether its text includes {expected:?}. Do not inspect another window or take screenshots."
        );
        let Output::QueuedTurn(turn) = client
            .execute(client.prepare(Command::SubmitTurn {
                session: session.id,
                expected_revision: session.revision,
                message: message.into(),
            }))
            .await
            .unwrap()
        else {
            panic!("read-only turn expected")
        };
        let page = complete(&client, &relay, session.id, turn.id).await;
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        pages.push((remote, turn.id, page));
        if relay.refused() {
            break;
        }
    }
    let report = relay.close().await;
    assert!(
        report["refusal"].is_null(),
        "read-only acceptance request refused after {} / {} attempts: {}",
        report["upstream_attempts"],
        relay::CAP,
        report["refusal"],
    );
    assert!(report["upstream_attempts"].as_u64().unwrap() <= relay::CAP as u64);
    assert_eq!(pages.len(), 2, "local and paired acceptance required");
    for (remote, turn, page) in pages {
        let run = page.runs.iter().find(|run| run.turn == turn).unwrap();
        assert_eq!(
            run.status,
            Status::Completed,
            "read-only turn failed, remote={remote}"
        );
        assert!(
            page.approvals.is_empty(),
            "read-only turn cannot require approval"
        );
        let parts: Vec<_> = page.entries.iter().flat_map(|entry| &entry.parts).collect();
        let mut observed = Vec::new();
        for part in &parts {
            if let Part::ToolCall {
                name, arguments, ..
            } = part
            {
                assert!(
                    arguments.get("session").is_none(),
                    "host-bound session cannot be selected"
                );
                assert!(
                    OBSERVATIONS.contains(&name.as_str()) || name == "set_session_title",
                    "unexpected read-only acceptance tool"
                );
            }
            if let Part::ToolResult {
                name,
                result,
                images,
                ..
            } = part
                && OBSERVATIONS.contains(&name.as_str())
            {
                assert_ne!(result["isError"], true, "native observation failed: {name}");
                assert!(
                    result.get("error").is_none(),
                    "native observation failed: {name}"
                );
                assert!(
                    images.is_empty(),
                    "read-only acceptance cannot capture screenshots"
                );
                if name == "get_window_state" {
                    assert!(result["structuredContent"]["snapshot_id"].is_string());
                    assert!(result["structuredContent"]["elements"].is_array());
                    assert!(
                        result["structuredContent"].to_string().contains(expected),
                        "owned fixture text missing"
                    );
                } else {
                    assert!(result["structuredContent"]["apps"].is_array());
                }
                observed.push(name.as_str());
            }
        }
        for name in OBSERVATIONS {
            assert!(
                observed.contains(&name),
                "native observation missing: {name}"
            );
        }
        println!(
            "read-only computer acceptance: remote={remote}, status={:?}, tools={observed:?}",
            run.status
        );
    }
    let final_state = state(Path::new(&fixture));
    for field in ["text", "clicks", "pointer_clicks", "unsupported_presses"] {
        assert!(
            final_state[field] == initial[field],
            "read-only acceptance changed owned fixture field: {field}"
        );
    }
    println!(
        "read-only model requests: {} / {}",
        report["upstream_attempts"],
        relay::CAP
    );
}
