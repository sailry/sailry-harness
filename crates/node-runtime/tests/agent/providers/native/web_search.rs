use super::*;
use crate::agent_support::{Server as SearchServer, web_search as source};

fn citations(page: &Page) {
    let entry = page
        .entries
        .iter()
        .find(|entry| !entry.citations.is_empty())
        .unwrap();
    assert_eq!(entry.citations.len(), 2);
    let first = source::FIRST.chars().count() as u32;
    assert_eq!(
        entry.citations[0],
        Citation {
            uri: source::URI.into(),
            title: Some("Fixture source".into()),
            start: Some(0),
            end: Some(first),
        }
    );
    assert_eq!(entry.citations[1].start, Some(first));
    assert_eq!(
        entry.citations[1].end,
        Some(first + source::SECOND.chars().count() as u32)
    );
    assert!(matches!(&entry.parts[0], Part::ToolCall { name, .. } if name == "web_search"));
    assert!(matches!(&entry.parts[1], Part::Text(text) if text == source::FIRST));
    assert!(matches!(&entry.parts[2], Part::Text(text) if text == source::SECOND));
    assert!(
        matches!(entry.parts.last().unwrap(), Part::ToolResult { name, result, .. }
        if name == "web_search" && result["status"] == "completed")
    );
}

#[tokio::test]
async fn combines_search_and_tools() {
    for remote in [false, true] {
        for tools in [false, true] {
            let server = SearchServer::web_search().await;
            let mut fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
            fixture.provider.models[0].web_search = true;
            fixture.provider.models[0].tools = tools;
            std::fs::write(fixture.root.join("native.txt"), "Search companion content").unwrap();
            execute(
                &fixture.client,
                Command::PutProvider {
                    expected_revision: fixture.provider.revision,
                    provider: fixture.provider.clone(),
                },
            )
            .await;
            let turn = fixture.submit("Find the fixture source").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            citations(&page);
            {
                let requests = server.requests.lock().unwrap();
                assert_eq!(requests.len(), if tools { 2 } else { 1 });
                let declarations = requests[0]["tools"].as_array().unwrap();
                assert!(declarations.iter().any(|tool| tool["type"] == "web_search"));
                assert_eq!(
                    declarations
                        .iter()
                        .any(|tool| tool["name"] == plugin_tool("files", "read_file")),
                    tools
                );
                assert!(requests.iter().all(|request| request["stream"] == true));
                if tools {
                    assert!(
                        requests[1]["input"]
                            .to_string()
                            .contains("Search companion content")
                    );
                    let entry = page
                        .entries
                        .iter()
                        .find(|entry| !entry.citations.is_empty())
                        .unwrap();
                    assert!(
                        matches!(&entry.parts[3], Part::ToolCall { name, .. } if name == &plugin_tool("files", "read_file"))
                    );
                }
            }
            let profile = fixture.node.profile().to_owned();
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            let node = Node::start(profile).await.unwrap();
            assert_eq!(
                history(&Client::new(node.local()), fixture.session.id).await,
                page
            );
            assert_eq!(
                server.requests.lock().unwrap().len(),
                if tools { 2 } else { 1 }
            );
            node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn freezes_queued_settings() {
    for remote in [false, true] {
        let server = SearchServer::web_search().await;
        let mut fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
        fixture.provider.models[0].web_search = true;
        fixture.provider.models[0].tools = false;
        let Output::Provider(mut provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: fixture.provider.revision,
                provider: fixture.provider.clone(),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let Output::QueuedTurn(queued) = execute(
            &fixture.client,
            Command::QueueTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Search before settings change".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        provider.models[0].web_search = false;
        execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await;
        let start = fixture
            .client
            .prepare(Command::StartQueuedTurn { turn: queued.id });
        fixture.client.execute(start.clone()).await.unwrap();
        let page = finished(&fixture.client, fixture.session.id, queued.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        citations(&page);
        fixture.client.execute(start).await.unwrap();
        let turn = fixture.submit("Continue without search").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == turn)
                .all(|entry| entry.citations.is_empty())
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert!(
            !requests[1]["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| tool["type"] == "web_search"))
        );
        assert!(
            requests[1]["input"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["type"] == "web_search_call")
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn rejects_unavailable_search() {
    for remote in [false, true] {
        let server = SearchServer::web_search().await;
        let mut fixture = Fixture::new(remote, ModelApi::ChatCompletions, &server.endpoint).await;
        fixture.provider.models[0].web_search = true;
        execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: fixture.provider.revision,
                provider: fixture.provider.clone(),
            },
        )
        .await;
        let turn = fixture.submit("Search is required").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        let run = page.runs.last().unwrap();
        assert_eq!(run.status, Status::Failed);
        assert_eq!(run.error.as_ref().unwrap().code, ErrorCode::NotConfigured);
        assert!(server.requests.lock().unwrap().is_empty());
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn supports_compatible_endpoints() {
    for remote in [false, true] {
        let server = SearchServer::web_search().await;
        let mut fixture = Fixture::new(remote, ModelApi::Responses, &server.endpoint).await;
        let model = "gpt-5.6-luna";
        fixture.provider.models[0].id = model.into();
        fixture.provider.default_model = model.into();
        fixture.provider.models[0].tools = false;
        assert!(!fixture.provider.models[0].web_search);
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: fixture.provider.revision,
                provider: fixture.provider.clone(),
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        assert!(provider.models[0].web_search);
        let mut config = fixture.session.config.clone();
        config.model = model.into();
        let Output::Session(session) = execute(
            &fixture.client,
            Command::SetSessionConfig {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                config,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = session;
        let turn = fixture.submit("Find the fixture source").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        citations(&page);
        assert!(
            server.requests.lock().unwrap()[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["type"] == "web_search")
        );
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn delivers_text_before_native_response_completes() {
    for remote in [false, true] {
        let finish = Arc::new(tokio::sync::Notify::new());
        let server = SearchServer::web_search_held(finish.clone()).await;
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
        let mut updates = fixture
            .client
            .subscribe_conversation(fixture.session.id)
            .await
            .unwrap();
        let turn = fixture.submit("Stream the sourced answer").await;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Update::ConversationFrame(frame) = updates.next().await.unwrap()
                    && let sailry_protocol::conversation::Change::Delta(draft) = frame.change
                    && draft.parts.iter().any(
                        |part| matches!(part, Part::Text(text) if text.contains(source::FIRST)),
                    )
                {
                    break;
                }
            }
        })
        .await
        .expect("text must arrive while response.completed is held");
        assert_eq!(server.requests.lock().unwrap()[0]["stream"], true);
        assert_eq!(
            history(&fixture.client, fixture.session.id)
                .await
                .runs
                .last()
                .unwrap()
                .status,
            Status::Running
        );
        finish.notify_one();
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        citations(&page);
        assert_eq!(text(&page).matches(source::FIRST).count(), 1);
        drop(updates);
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
