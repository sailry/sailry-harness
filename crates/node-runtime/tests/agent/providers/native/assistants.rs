use super::*;
use crate::agent_support::Server as SearchServer;
use sailry_protocol::plugin::conversation::Binding;
use serde_json::json;

async fn setup(remote: bool, api: ModelApi, endpoint: &str, allow_search: bool) -> Fixture {
    let mut fixture = Fixture::packages(remote, api, endpoint, &[]).await;
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
            expected_digest: None,
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
