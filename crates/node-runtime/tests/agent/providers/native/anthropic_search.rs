use super::*;
use crate::agent_support::{Server as SearchServer, anthropic, web_search as source};

async fn setup(remote: bool, reply: anthropic::Reply, tools: bool) -> (Fixture, SearchServer) {
    let server = SearchServer::anthropic(reply).await;
    let mut fixture = Fixture::new(remote, ModelApi::Anthropic, &server.endpoint).await;
    fixture.provider.models[0].web_search = true;
    fixture.provider.models[0].tools = tools;
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
    fixture.provider = provider;
    std::fs::write(fixture.root.join("native.txt"), "Search companion content").unwrap();
    (fixture, server)
}

fn citations(page: &Page) {
    let entry = page
        .entries
        .iter()
        .find(|entry| !entry.citations.is_empty())
        .unwrap();
    assert_eq!(entry.citations.len(), 2);
    assert_eq!(entry.citations[0].start, Some(0));
    let first = source::FIRST.chars().count() as u32;
    assert_eq!(entry.citations[0].end, Some(first));
    assert_eq!(entry.citations[1].start, Some(first));
    assert_eq!(
        entry.citations[1].end,
        Some(first + source::SECOND.chars().count() as u32)
    );
    assert!(
        entry
            .citations
            .iter()
            .all(|citation| citation.uri == source::URI
                && citation.title.as_deref() == Some("Fixture source"))
    );
    let public = serde_json::to_string(page).unwrap();
    assert!(!public.contains(anthropic::ENCRYPTED));
    assert!(!public.contains(anthropic::INDEX));
    assert!(!public.contains("anthropic_message"));
}

fn assistant(request: &serde_json::Value) -> &serde_json::Value {
    &request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .unwrap()["content"]
}

#[tokio::test]
async fn preserves_history_across_adapters() {
    use adk_core::{Content, Llm, LlmRequest};
    use adk_model::anthropic::{AnthropicClient, AnthropicConfig};
    use futures::StreamExt;
    for stream in [false, true] {
        for reply in [anthropic::Reply::Search, anthropic::Reply::SearchError] {
            let server = SearchServer::anthropic(reply).await;
            let model = AnthropicClient::new(
                AnthropicConfig::new("fixture-key", "fixture-a").with_base_url(&server.endpoint),
            )
            .unwrap();
            let mut request = LlmRequest::new("fixture-a", vec![]);
            request
                .contents
                .push(Content::new("user").with_text("Find a source"));
            request.tools.insert(
                "web_search".into(),
                serde_json::json!({"x-adk-anthropic-tool":{
                "type":"web_search_20250305", "name":"web_search"}}),
            );
            let responses = model
                .generate_content(request.clone(), stream)
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<adk_core::Result<Vec<_>>>()
                .unwrap();
            let response = responses.last().unwrap();
            assert!(response.turn_complete);
            let content = response.content.as_ref().unwrap();
            let texts: Vec<_> = content
                .parts
                .iter()
                .filter_map(|part| match part {
                    adk_core::Part::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(texts, vec![source::FIRST, source::SECOND]);
            request.contents.push(content.clone());
            request
                .contents
                .push(Content::new("user").with_text("Continue"));
            model
                .generate_content(request, stream)
                .await
                .unwrap()
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<adk_core::Result<Vec<_>>>()
                .unwrap();
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(
                assistant(&requests[1]),
                &serde_json::json!(anthropic::blocks(&requests[0], reply).0)
            );
        }
    }
}

#[tokio::test]
async fn round_trips_sources_with_tools() {
    for remote in [false, true] {
        for tools in [false, true] {
            let (fixture, server) = setup(remote, anthropic::Reply::Search, tools).await;
            let turn = fixture.submit("Find a source").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            citations(&page);
            assert!(
                page.entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| {
                        matches!(part, Part::ToolCall { name, arguments, .. }
                    if name == "web_search" && arguments["query"] == "fixture source 🙂")
                    })
            );
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), if tools { 2 } else { 1 });
            assert_eq!(requests[0]["stream"], true);
            if tools {
                assert_eq!(
                    assistant(&requests[1]),
                    &serde_json::json!(anthropic::blocks(&requests[0], anthropic::Reply::Search).0)
                );
                assert!(
                    requests[1]["messages"]
                        .to_string()
                        .contains("Search companion content")
                );
            }
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn resumes_paused_search() {
    for remote in [false, true] {
        let (fixture, server) = setup(remote, anthropic::Reply::Pause, false).await;
        let turn = fixture.submit("Find a source").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        citations(&page);
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            assistant(&requests[1]),
            &serde_json::json!(anthropic::blocks(&requests[0], anthropic::Reply::Pause).0)
        );
        assert!(
            page.entries
                .iter()
                .flat_map(|entry| &entry.parts)
                .any(|part| {
                    matches!(part, Part::ToolResult { name, result, .. } if name == "web_search"
                && result["sources"][0]["url"] == source::URI)
                })
        );
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn restores_settings_and_encrypted_history() {
    for remote in [false, true] {
        let (mut fixture, server) = setup(remote, anthropic::Reply::Search, false).await;
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
        fixture.provider.models[0].web_search = false;
        execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: fixture.provider.revision,
                provider: fixture.provider.clone(),
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
        let profile = fixture.node.profile().to_owned();
        fixture.node.shutdown().await.unwrap();
        let node = Node::start(profile).await.unwrap();
        let address = fixture
            .controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            fixture.controller.handle().remote(address)
        } else {
            node.local()
        });
        assert_eq!(history(&client, fixture.session.id).await, page);
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: "Continue without search".into(),
            },
        )
        .await
        else {
            panic!("turn expected")
        };
        let page = finished(&client, fixture.session.id, turn.id).await;
        assert_eq!(
            page.runs.last().unwrap().status,
            Status::Completed,
            "{:?}",
            page.runs
        );
        assert!(
            page.entries
                .iter()
                .filter(|entry| entry.turn == turn.id)
                .all(|entry| entry.citations.is_empty())
        );
        let requests = server.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            assistant(&requests[1]),
            &serde_json::json!(anthropic::blocks(&requests[0], anthropic::Reply::Search).0)
        );
        assert!(
            !requests[1]["tools"]
                .to_string()
                .contains("web_search_20250305")
        );
        fixture.controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn reports_incomplete_search() {
    for remote in [false, true] {
        for reply in [
            anthropic::Reply::SearchError,
            anthropic::Reply::RequestTooLarge,
            anthropic::Reply::Truncated,
        ] {
            let (fixture, server) = setup(remote, reply, false).await;
            let turn = fixture.submit("Find a source").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            if matches!(reply, anthropic::Reply::Truncated) {
                assert_eq!(page.runs.last().unwrap().status, Status::Failed);
            } else {
                assert_eq!(page.runs.last().unwrap().status, Status::Completed);
                assert!(page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
                    matches!(part, Part::ToolResult { name, result, .. } if name == "web_search"
                        && result.get("error").is_some())
                }));
            }
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
        }
    }
}
