use super::*;
use serde_json::json;

const PACKAGE: &str = "turn-messages";
const SOURCE: &str = r#"
export function initialize({tools,message}) {
  return {tools, instruction:`\n\nObserved message: ${JSON.stringify(message)}`};
}
export function note() { return {noted:true}; }
"#;

async fn install(fixture: &process::Fixture, declared: bool) {
    let root = fixture.root.join("package");
    std::fs::create_dir_all(root.join("dev.sailry.platform/host")).unwrap();
    std::fs::write(root.join("dev.sailry.platform/host/main.js"), SOURCE).unwrap();
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
