use super::*;
use serde_json::json;

const PACKAGE: &str = "turn-messages";
const SOURCE: &str = r#"
export function initialize({tools,message}) {
  return {tools, instruction:`\n\nObserved message: ${JSON.stringify(message)}`};
}
export function note() { return {noted:true}; }
"#;

const PARTS: &str = r#"
export function initialize({tools,message}) {
  const names = message?.startsWith("parts:") ? message.slice(6).split("|") : [];
  return {tools, instruction:"", parts:names.map(name => `Fixture part ${name}`)};
}
export function note() { return {noted:true}; }
"#;
const MARK: &str = "Fixture part ";

async fn install(fixture: &process::Fixture, declared: bool) {
    install_source(fixture, declared, SOURCE).await;
}

async fn install_source(fixture: &process::Fixture, declared: bool, source: &str) {
    let root = fixture.root.join("package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), source).unwrap();
    let mut actions = vec!["storage.read"];
    if declared {
        actions.push("conversation.message.read");
    }
    std::fs::write(root.join("plugin.json"), json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":PACKAGE,"version":"0.1.0","extensions":{"dev.sailry.platform":{
            "api_version":"v1","actions":actions,
            "host":{"entry":"dev.sailry.platform/host/main.js","resources":["dev.sailry.platform/host/main.js"],
                "handlers":["initialize","note"],"turn":{"initialize":"initialize"}},
            "tools":[{"name":"note","description":"Record a note","initial":true,"handler":{
                "name":"note","read_only":true,"parameters":{"type":"object","properties":{},"additionalProperties":false,"required":[]}
            }}]
        }}
    }).to_string()).unwrap();
    let Output::Plugin(package) = fixture
        .client
        .execute(fixture.client.prepare(Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "package".into(),
            name: PACKAGE.into(),
            expected_revision: 0,
            expected_digest: None,
        }))
        .await
        .unwrap()
    else {
        panic!("package expected")
    };
    assert!(package.issues.is_empty(), "{:?}", package.issues);
}

#[tokio::test]
async fn shares_capped_text_only_when_declared() {
    for remote in [false, true] {
        for declared in [false, true] {
            let server = Server::tools(vec![]).await;
            let fixture = process::Fixture::packages(remote, &server, &[]).await;
            install(&fixture, declared).await;
            // 6,000 UTF-8 bytes are cut to the last whole character within 4 KiB.
            let text = "记".repeat(2000);
            let Output::QueuedTurn(turn) = fixture
                .client
                .execute(fixture.client.prepare(Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: text.clone().into(),
                }))
                .await
                .unwrap()
            else {
                panic!("turn expected")
            };
            let page = finished(&fixture.client, fixture.session.id, turn.id).await;
            assert_eq!(page.runs[0].status, Status::Completed, "{page:?}");
            let request = server.requests.lock().unwrap()[0].clone();
            let context = turn_context(&request);
            if declared {
                let excerpt = "记".repeat(1365);
                assert!(context.contains(&format!("Observed message: \"{excerpt}\"")));
                assert!(!context.contains(&"记".repeat(1366)));
            } else {
                assert!(context.contains("Observed message: null"));
            }
            let names: Vec<_> = request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| tool["function"]["name"].as_str())
                .collect();
            assert!(names.contains(&exposed(&plugin_tool(PACKAGE, "note"))));
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}

/// Runs one turn, then reports its request's latest persisted context and how
/// often a fixture part appears anywhere in that request.
async fn turn(fixture: &process::Fixture, server: &Server, message: &str) -> (String, usize) {
    let Output::QueuedTurn(queued) = fixture
        .client
        .execute(fixture.client.prepare(Command::SubmitTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: message.into(),
        }))
        .await
        .unwrap()
    else {
        panic!("turn expected")
    };
    let page = finished(&fixture.client, fixture.session.id, queued.id).await;
    let run = page.runs.iter().find(|run| run.turn == queued.id).unwrap();
    assert_eq!(run.status, Status::Completed, "{page:?}");
    let requests = server.requests.lock().unwrap();
    let request = requests.last().unwrap();
    let latest = turn_contexts(request).last().copied().unwrap_or_default();
    (
        latest.to_owned(),
        request["messages"].to_string().matches(MARK).count(),
    )
}

#[tokio::test]
async fn states_each_part_once_per_conversation() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        install_source(&fixture, true, PARTS).await;
        let (latest, total) = turn(&fixture, &server, "parts:alpha|beta").await;
        assert!(latest.contains("Fixture part alpha"), "{latest}");
        assert!(latest.contains("Fixture part beta"), "{latest}");
        assert_eq!(total, 2);
        // A repeated part is skipped and a new one is added beside it.
        let (latest, total) = turn(&fixture, &server, "parts:alpha|gamma").await;
        assert!(latest.contains("Fixture part gamma"), "{latest}");
        assert!(!latest.contains("Fixture part alpha"), "{latest}");
        assert_eq!(total, 3);
        let (_, total) = turn(&fixture, &server, "parts:gamma|beta|alpha").await;
        assert_eq!(total, 3);
        let (_, total) = turn(&fixture, &server, "no parts").await;
        assert_eq!(total, 3);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_distinct_substring_parts() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        install_source(&fixture, true, PARTS).await;
        let (latest, total) = turn(&fixture, &server, "parts:alpha extended").await;
        assert!(latest.contains("Fixture part alpha extended"), "{latest}");
        assert_eq!(total, 1);
        let (latest, total) = turn(&fixture, &server, "parts:alpha").await;
        assert!(latest.contains("Fixture part alpha"), "{latest}");
        assert!(!latest.contains("Fixture part alpha extended"), "{latest}");
        assert_eq!(total, 2);
        let (_, total) = turn(&fixture, &server, "parts:alpha extended|alpha").await;
        assert_eq!(total, 2);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn keeps_system_prompt_across_part_changes() {
    for remote in [false, true] {
        let server = Server::tools(vec![]).await;
        let fixture = process::Fixture::packages(remote, &server, &[]).await;
        install_source(&fixture, true, PARTS).await;
        for message in ["parts:alpha", "parts:beta|gamma", "no parts", "parts:alpha"] {
            turn(&fixture, &server, message).await;
        }
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 4);
        let system = |request: &serde_json::Value| -> String {
            request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| matches!(message["role"].as_str(), Some("system" | "developer")))
                .map(|message| message_text(&message["content"]))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let first = system(&requests[0]);
        assert!(!first.is_empty());
        assert!(!first.contains(MARK));
        for request in requests.iter() {
            assert_eq!(system(request), first);
        }
        drop(requests);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
