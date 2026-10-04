use super::*;
use crate::agent_support::Server as SearchServer;

async fn prepare(remote: bool, server: &SearchServer) -> Fixture {
    let mut fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
    fixture.provider.models[0].web_search = true;
    fixture.provider.models[0].tools = false;
    execute(
        &fixture.client,
        Command::PutProvider {
            expected_revision: fixture.provider.revision,
            provider: fixture.provider.clone(),
        },
    )
    .await;
    fixture
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
    manifest["extensions"]["dev.sailry.platform"]["model_tools"][0]["display"]["label"] =
        serde_json::json!(label);
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
            Part::ToolCall {
                name,
                display: Some(display),
                ..
            } if name == "web_search" => Some(display.label.as_str()),
            _ => None,
        })
        .expect("search display expected")
}

#[tokio::test]
async fn retains_admitted_version_after_update_and_removal() {
    for remote in [false, true] {
        let server = SearchServer::web_search().await;
        let fixture = prepare(remote, &server).await;
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
                message: "Find sources".into(),
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
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(profile).await.unwrap();
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
        let server = SearchServer::web_search().await;
        let fixture = prepare(remote, &server).await;
        source(&fixture, "web-search", "Changed search");
        install(&fixture, "web-search", 1).await;
        let updated = fixture.submit("Use the updated search package").await;
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
                        |part| !matches!(part, Part::ToolCall { name, .. } if name == "web_search")
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
        let enabled = fixture.submit("Search again").await;
        let page = finished(&fixture.client, fixture.session.id, enabled).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(label(&page, enabled), "Changed search");
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 3);
        for (index, request) in requests.iter().enumerate() {
            assert_eq!(
                request["tools"]
                    .as_array()
                    .is_some_and(|tools| tools.iter().any(|tool| tool["type"] == "web_search")),
                index != 1
            );
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn ordinary_packages_share_one_provider_capability() {
    for remote in [false, true] {
        let server = SearchServer::web_search().await;
        let fixture = prepare(remote, &server).await;
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
        let turn = fixture.submit("Find a source").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        assert_eq!(label(&page, turn), "Find sources");
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|tool| tool["type"] == "web_search")
                .count(),
            1
        );
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
