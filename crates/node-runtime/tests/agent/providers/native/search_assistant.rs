use super::*;
use crate::agent_support::Server as SearchServer;
use sailry_protocol::plugin::conversation::Binding;
use serde_json::json;
#[path = "../../../support/search/mcp.rs"]
mod http;

const OWNER: &str = "search-assistant";
const TOOL: &str = "lookup";

async fn setup(remote: bool, endpoint: &str, native: bool) -> Fixture {
    let mut fixture = Fixture::new(remote, ModelApi::Responses, endpoint).await;
    fixture.provider.models[0].web_search = !native;
    fixture.provider.models[0]
        .capabilities
        .get_or_insert_with(Default::default)
        .web_search = Some(native);
    execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: fixture.provider.revision,
            provider: fixture.provider.clone(),
        },
    )
    .await;
    let root = fixture.root.join("assistant");
    std::fs::create_dir_all(root.join(plugin::NAMESPACE)).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/main.js"),
        "export default {};",
    )
    .unwrap();
    std::fs::write(root.join("plugin.json"),json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":OWNER,"version":"0.1.0",
        "extensions":{"dev.sailry.platform":{
            "api_version":"v1","scope":"host","actions":["conversation.read","conversation.control","web.read"],
            "model_tools":[{"capability":"web_search","display":{"label":"Assistant search"}}],
            "tools":[{"name":TOOL,"operation":"web.search"},{"name":"hidden_fetch","operation":"web.fetch"}],
            "desktop":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
                "conversations":[{"id":"search","resource":"workspace","context":"Find sources",
                    "tools":[{"kind":"model","capability":"web_search"}]}]}
        }}
    }).to_string()).unwrap();
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "assistant".into(),
            name: OWNER.into(),
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("assistant plugin expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    let mut config = fixture.session.config.clone();
    config.assistant = Some(Binding {
        package: info.summary.reference(),
        id: "search".into(),
    });
    let Output::Session(session) = execute(
        &fixture.client,
        Command::CreateSession {
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            config: Some(config),
        },
    )
    .await
    else {
        panic!("assistant session expected")
    };
    fixture.session = session;
    fixture
}

#[tokio::test]
async fn chooses_one_declared_search_and_executes_fallback() {
    for remote in [false, true] {
        for native in [false, true] {
            let name = plugin_tool(OWNER, TOOL);
            let model = if native {
                SearchServer::web_search().await
            } else {
                SearchServer::phases_with_tool(&name, json!({"query":"Public source","limit":1}))
                    .await
            };
            let http = http::Server::start().await;
            let fixture = setup(remote, &model.endpoint, native).await;
            fixture.node.set_search_fixture(
                sailry_protocol::search::Driver::Exa,
                &format!("{}/mcp", http.endpoint),
            );
            let turn = fixture.submit("Find a public source").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            assert!(page.questions.is_empty());
            let requests = model.requests.lock().unwrap().clone();
            let tools = requests[0]["tools"].as_array().unwrap();
            assert_eq!(
                tools
                    .iter()
                    .filter(|tool| tool["type"] == "web_search")
                    .count(),
                usize::from(native)
            );
            assert_eq!(
                tools.iter().filter(|tool| tool["name"] == name).count(),
                usize::from(!native)
            );
            assert!(!tools.iter().any(|tool| tool["name"]
                == plugin_tool("web-search", "web_search")
                || tool["name"] == plugin_tool(OWNER, "hidden_fetch")
                || tool["name"] == plugin_tool("files", "read_file")));
            if native {
                assert_eq!(http.count("/mcp"), 0);
            } else {
                assert_eq!(
                    http.requests
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|(_, body)| body["method"] == "tools/call")
                        .count(),
                    1
                );
                let output = page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .find_map(|part| {
                        if let Part::ToolResult {
                            name: tool, result, ..
                        } = part
                            && tool == &name
                        {
                            Some(result)
                        } else {
                            None
                        }
                    })
                    .expect("fallback result expected");
                assert_eq!(output["driver"], "exa", "{output}");
                assert_eq!(output["untrusted"], true);
                assert_eq!(output["results"][0]["url"], "https://example.com/source");
                assert_eq!(output["results"].as_array().unwrap().len(), 1);
                assert!(
                    requests.last().unwrap()["input"]
                        .to_string()
                        .contains("Public source")
                );
            }
            fixture.node.shutdown().await.unwrap();
            fixture.controller.close().await.unwrap();
        }
    }
}
