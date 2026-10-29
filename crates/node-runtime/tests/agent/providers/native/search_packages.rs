use super::search_http as http;
use super::*;
use crate::agent_support::Server as SearchServer;

async fn prepare(remote: bool, server: &SearchServer) -> (Fixture, http::Server) {
    let mut fixture = Fixture::packages(
        remote,
        ModelApi::ChatCompletions,
        &server.endpoint,
        &["web-search"],
    )
    .await;
    fixture.provider.models[0]
        .capabilities
        .get_or_insert_with(Default::default)
        .tools = Some(true);
    execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: fixture.provider.revision,
            provider: fixture.provider.clone(),
        },
    )
    .await;
    let http = http::Server::start().await;
    fixture.node.set_search_fixture(
        sailry_protocol::search::Driver::Exa,
        &format!("{}/mcp", http.endpoint),
    );
    (fixture, http)
}

fn source(fixture: &Fixture, name: &str, label: &str) {
    let root = fixture.root.join(name);
    std::fs::create_dir_all(&root).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../plugins/web-search/plugin.json"
    ))
    .unwrap();
    manifest["name"] = serde_json::json!(name);
    manifest["version"] = serde_json::json!("0.2.0");
    let tool = manifest["extensions"]["dev.sailry.platform"]["tools"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "web_search")
        .unwrap();
    tool["display"]["label"] = serde_json::json!(label);
    std::fs::write(root.join("plugin.json"), manifest.to_string()).unwrap();
}

async fn install(fixture: &Fixture, name: &str, revision: u64) -> plugin::Info {
    let Output::Plugin(info) = execute(
        &fixture.client,
        Command::InstallPlugin {
            worktree: fixture.session.worktree,
            path: name.into(),
            name: name.into(),
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    else {
        panic!("package expected")
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

fn label(page: &Page, turn: TurnId) -> &str {
    page.entries
        .iter()
        .filter(|entry| entry.turn == turn)
        .flat_map(|entry| &entry.parts)
        .find_map(|part| match part {
            Part::ToolCall { name, summary, .. }
                if name == &plugin_tool("web-search", "web_search") =>
            {
                summary
                    .resolved
                    .as_ref()?
                    .label
                    .as_ref()
                    .map(|label| label.label.as_str())
            }
            _ => None,
        })
        .expect("search display expected")
}

#[tokio::test]
async fn retains_admitted_version_after_update_and_removal() {
    for remote in [false, true] {
        let server = SearchServer::prompt_tools(
            "Search".into(),
            vec![(
                plugin_tool("web-search", "web_search"),
                serde_json::json!({"query":"Public source","limit":1}),
            )],
        )
        .await;
        let (fixture, _http) = prepare(remote, &server).await;
        let Output::Plugin(original) = execute(
            &fixture.client,
            Command::ReadPlugin {
                name: "web-search".into(),
            },
        )
        .await
        else {
            panic!("package expected")
        };
        assert_eq!(original.summary.revision, 1);
        assert_eq!(original.summary.digest.len(), 64);
        let Output::QueuedTurn(queued) = execute(
            &fixture.client,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Search".into(),
            },
        )
        .await
        else {
            panic!("queued turn expected")
        };
        assert_eq!(
            queued
                .plugins
                .iter()
                .find(|reference| reference.name == "web-search"),
            Some(&original.summary.reference())
        );
        source(&fixture, "web-search", "Changed search");
        let updated = install(&fixture, "web-search", 1).await;
        assert_ne!(original.summary.digest, updated.summary.digest);
        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "web-search".into(),
                expected_revision: 2,
                enabled: false,
            },
        )
        .await;
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: "web-search".into(),
                expected_revision: 3,
            },
        )
        .await;
        execute(
            &fixture.client,
            Command::StartQueuedTurn { turn: queued.id },
        )
        .await;
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(label(&page, queued.id), "Search web");
        let next = fixture.submit("Continue without search").await;
        let page = finished(&fixture.client, fixture.session.id, next).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == next)
                .all(|entry| entry.citations.is_empty())
        );
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start_empty(profile).await.unwrap();
        let restored = history(&Client::new(node.local()), fixture.session.id).await;
        assert_eq!(restored, page);
        assert_eq!(label(&restored, queued.id), "Search web");
        node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn new_turns_follow_updates_and_enablement() {
    for remote in [false, true] {
        let server = SearchServer::prompt_tools(
            "Search".into(),
            vec![(
                plugin_tool("web-search", "web_search"),
                serde_json::json!({"query":"Public source","limit":1}),
            )],
        )
        .await;
        let (fixture, _http) = prepare(remote, &server).await;
        source(&fixture, "web-search", "Changed search");
        install(&fixture, "web-search", 1).await;
        let updated = fixture.submit("Search").await;
        let page = finished(&fixture.client, fixture.session.id, updated).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(label(&page, updated), "Changed search");

        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "web-search".into(),
                expected_revision: 2,
                enabled: false,
            },
        )
        .await;
        let disabled = fixture.submit("Continue while search is disabled").await;
        let page = finished(&fixture.client, fixture.session.id, disabled).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == disabled)
                .all(|entry| entry.citations.is_empty()
                    && entry.parts.iter().all(
                        |part| !matches!(part, Part::ToolCall { name, .. } if name == &plugin_tool("web-search", "web_search"))
                    ))
        );
        assert_eq!(label(&page, updated), "Changed search");

        execute(
            &fixture.client,
            Command::SetPluginEnabled {
                name: "web-search".into(),
                expected_revision: 3,
                enabled: true,
            },
        )
        .await;
        let enabled = fixture.submit("Search").await;
        let page = finished(&fixture.client, fixture.session.id, enabled).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(label(&page, enabled), "Changed search");
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 5);
        for (index, request) in requests.iter().enumerate() {
            assert_eq!(
                request["tools"]
                    .as_array()
                    .is_some_and(|tools| tools
                        .iter()
                        .any(|tool| tool["function"]["name"]
                            == plugin_tool("web-search", "web_search"))),
                index != 2
            );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn ordinary_packages_share_one_fallback_tool() {
    for remote in [false, true] {
        let server = SearchServer::start(false).await;
        let fixture = Fixture::packages(
            remote,
            ModelApi::Responses,
            &server.endpoint,
            &["web-search"],
        )
        .await;
        execute(
            &fixture.client,
            Command::RemovePlugin {
                name: "web-search".into(),
                expected_revision: 1,
            },
        )
        .await;
        source(&fixture, "source-finder", "Find sources");
        install(&fixture, "source-finder", 0).await;
        source(&fixture, "source-helper", "Other sources");
        install(&fixture, "source-helper", 0).await;
        let turn = fixture.submit("Use the available search driver").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        {
            let requests = server.requests.lock().unwrap();
            let tools = requests[0]["tools"].as_array().unwrap();
            assert_eq!(
                tools
                    .iter()
                    .filter(|tool| {
                        tool["name"] == plugin_tool("source-finder", "web_search")
                            || tool["name"] == plugin_tool("source-helper", "web_search")
                    })
                    .count(),
                1
            );
            assert!(!tools.iter().any(|tool| tool["type"] == "web_search"));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
