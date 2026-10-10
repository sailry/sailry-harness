//! Manual, billable smoke coverage; no external benchmark or competitor score is implied.
use super::*;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    prompt: String,
    files: BTreeMap<String, String>,
}

#[tokio::test]
#[ignore = "requires explicit acceptance profile, provider and model; incurs model usage"]
async fn local_and_remote_conversations() {
    let model = std::env::var("SAILRY_ACCEPTANCE_MODEL").expect("explicit model required");
    let provider = std::env::var("SAILRY_ACCEPTANCE_PROVIDER_ID")
        .expect("explicit provider required")
        .parse()
        .unwrap();
    let cases: Vec<Case> = serde_json::from_str(include_str!("smoke/cases.json")).unwrap();
    let selected = std::env::var("SAILRY_SMOKE_CASE").ok();
    assert!(
        selected
            .as_ref()
            .is_none_or(|selected| cases.iter().any(|case| &case.name == selected)),
        "unknown smoke case"
    );
    for remote in [false, true] {
        for case in cases.iter().filter(|case| {
            selected
                .as_ref()
                .is_none_or(|selected| &case.name == selected)
        }) {
            let directory = tempfile::tempdir().unwrap();
            let root = directory.path().join("project");
            std::fs::create_dir(&root).unwrap();
            for (name, body) in &case.files {
                std::fs::write(root.join(name), body).unwrap();
            }
            let profile = directory.path().join("node");
            let node = Node::start(&profile).await.unwrap();
            let identity = node.id();
            node.shutdown().await.unwrap();
            let source = provider_fixture::seed(&model, provider, &profile, identity);
            let node = Node::start(&profile).await.unwrap();
            let controller =
                Link::controller(directory.path().join("controller"), NetworkScope::default())
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
            let (session, _) = configured(&client, &source.endpoint, &root).await;
            let mut config = session.config;
            config.model = model.clone();
            config.provider = source.id;
            config.credential = source.credential;
            config.effort = Effort::Low;
            config.permission = Permission::Full;
            let Output::Session(session) = client
                .execute(client.prepare(Command::SetSessionConfig {
                    session: session.id,
                    expected_revision: session.revision,
                    config,
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            let started = std::time::Instant::now();
            let Output::QueuedTurn(turn) = client
                .execute(client.prepare(Command::SubmitTurn {
                    session: session.id,
                    expected_revision: session.revision,
                    message: case.prompt.clone().into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = loop {
                let page = history(&client, session.id).await;
                if page.runs.iter().any(|run| {
                    run.turn == turn.id
                        && !matches!(
                            run.status,
                            Status::Queued | Status::Running | Status::Stopping
                        )
                }) {
                    break page;
                }
                if started.elapsed() > Duration::from_secs(180) {
                    client
                        .execute(client.prepare(Command::StopTurn { turn: turn.id }))
                        .await
                        .unwrap();
                    break finished(&client, session.id, turn.id).await;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            };
            let answer: String = page
                .entries
                .iter()
                .filter(|entry| entry.author != "user")
                .flat_map(|entry| &entry.parts)
                .filter_map(|part| {
                    if let Part::Text(text) = part {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            let passed = page.runs.last().unwrap().status == Status::Completed
                && verify(case, &root, &answer);
            let usage: Vec<_> = page
                .entries
                .iter()
                .filter_map(|entry| entry.usage.as_ref())
                .collect();
            println!(
                "{}",
                json!({"suite":"conversation-smoke","case":case.name,"model":model,"remote":remote,"passed":passed,"elapsed_ms":started.elapsed().as_millis(),"input_tokens":usage.iter().map(|usage|usage.input).sum::<u64>(),"output_tokens":usage.iter().map(|usage|usage.output).sum::<u64>()})
            );
            node.shutdown().await.unwrap();
            controller.close().await.unwrap();
            assert!(
                passed,
                "smoke outcome failed: {} remote={remote}",
                case.name
            );
        }
    }
}

fn verify(case: &Case, root: &std::path::Path, answer: &str) -> bool {
    if case
        .files
        .iter()
        .any(|(name, body)| std::fs::read(root.join(name)).ok().as_deref() != Some(body.as_bytes()))
    {
        return false;
    }
    match case.name.as_str() {
        "files" => {
            std::fs::read(root.join("output.txt")).ok().as_deref()
                == Some(case.files["input.txt"].as_bytes())
        }
        "chart" => {
            let Some(json) = answer
                .split_once("```chart")
                .and_then(|(_, rest)| rest.split_once("```"))
                .map(|(body, _)| body)
            else {
                return false;
            };
            let Ok(chart) = serde_json::from_str::<serde_json::Value>(json.trim()) else {
                return false;
            };
            chart["type"] == "bar"
                && chart["data"]
                    == json!([{"label":"Jan","value":12},{"label":"Feb","value":18},{"label":"Mar","value":15}])
        }
        "math" => {
            let display =
                answer.contains("$$") || (answer.contains("\\[") && answer.contains("\\]"));
            let inline = answer.contains("\\(") && answer.contains("\\)")
                || answer.replace("$$", "").matches('$').count() >= 2;
            let formula = answer.replace(['{', '}'], "");
            inline
                && display
                && formula.contains("a^2")
                && formula.contains("b^2")
                && formula.contains("c^2")
        }
        _ => false,
    }
}
