use super::*;
use crate::agent_support::{Server as SearchServer, anthropic};
use sailry_protocol::plugin::conversation::Binding;
use serde_json::json;

async fn setup(remote: bool, api: ModelApi, endpoint: &str, allow_search: bool) -> Fixture {
    let mut fixture = Fixture::new(remote, api, endpoint).await;
    fixture.provider.models[0].web_search = true;
    execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: fixture.provider.revision,
            provider: fixture.provider.clone(),
        },
    )
    .await;
    let root = fixture.root.join("assistant");
    std::fs::create_dir_all(root.join(sailry_protocol::plugin::NAMESPACE)).unwrap();
    std::fs::write(
        root.join("dev.sailry.platform/main.js"),
        "export default {};",
    )
    .unwrap();
    std::fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
            "name":"search-assistant", "version":"0.1.0",
            "extensions":{"dev.sailry.platform":{
                "api_version":"v1", "scope":"host",
                "actions":["conversation.read", "conversation.control"],
                "model_tools":[{"capability":"web_search", "display":{"label":"Assistant search"}}],
                "desktop":{
                    "entry":"dev.sailry.platform/main.js", "resources":["dev.sailry.platform/main.js"],
                    "conversations":[{
                        "id":"search", "resource":"workspace", "context":"Find sources",
                        "tools": if allow_search { json!([
                            {"kind":"model", "capability":"web_search"}
                        ]) } else { json!([]) }
                    }]
                }
            }}
        })
        .to_string(),
    )
    .unwrap();
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: "assistant".into(),
            name: "search-assistant".into(),
            expected_revision: 0,
        },
    )
    .await
    else {
        panic!("plugin expected")
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
        panic!("session expected")
    };
    fixture.session = session;
    fixture
}

async fn run(fixture: &Fixture) {
    let turn = fixture.submit("Find a source").await;
    let page = finished(&fixture.client, fixture.session.id, turn).await;
    assert_eq!(
        page.runs.last().unwrap().status,
        Status::Completed,
        "{:?}",
        page.runs
    );
    assert!(page.entries.iter().any(|entry| !entry.citations.is_empty()));
}

#[tokio::test]
async fn openai() {
    for remote in [false, true] {
        let server = SearchServer::web_search().await;
        let fixture = setup(remote, ModelApi::Responses, &server.endpoint, true).await;
        run(&fixture).await;
        let requests = server.requests.lock().unwrap().clone();
        let tools = requests[0]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["type"] == "web_search"));
        assert!(
            !tools
                .iter()
                .any(|tool| tool["name"] == plugin_tool("files", "read_file"))
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn anthropic() {
    for remote in [false, true] {
        let server = SearchServer::anthropic(anthropic::Reply::Search).await;
        let fixture = setup(remote, ModelApi::Anthropic, &server.endpoint, true).await;
        run(&fixture).await;
        let requests = server.requests.lock().unwrap().clone();
        let tools = requests[0]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "web_search"));
        assert!(
            !tools
                .iter()
                .any(|tool| tool["name"] == plugin_tool("files", "read_file"))
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn gemini() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Gemini, Reply::Grounding).await;
        let fixture = setup(remote, ModelApi::Gemini, &server.endpoint, true).await;
        run(&fixture).await;
        let requests = server.requests.lock().unwrap().clone();
        let tools = requests[0].body["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["google_search"].is_object()));
        assert!(!serde_json::to_string(tools).unwrap().contains("read_file"));
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn excluded_capabilities_do_not_require_provider_support() {
    for remote in [false, true] {
        let server = SearchServer::start(false).await;
        let fixture = setup(remote, ModelApi::ChatCompletions, &server.endpoint, false).await;
        let turn = fixture.submit("Answer without search").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert!(
            !requests[0]["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| [
                    "openai_web_search",
                    "web_search",
                    "google_search"
                ]
                .contains(&tool["function"]["name"].as_str().unwrap_or(""))))
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
