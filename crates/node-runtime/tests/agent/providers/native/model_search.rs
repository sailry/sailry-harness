use super::*;
use crate::agent_support::Server as MainServer;
use sailry_protocol::search;
use serde_json::{Value, json};

#[path = "../../../support/search/model.rs"]
mod native;

#[path = "model_search/cancellation.rs"]
mod cancellation;

async fn report(
    client: &Client,
    session: &Session,
    provider: &Provider,
) -> sailry_protocol::usage::Report {
    let now = chrono::Utc::now().timestamp_millis();
    let Output::Usage(report) = execute(
        client,
        Command::ReadUsage(sailry_protocol::usage::Query {
            start_ms: now - 60_000,
            end_ms: now + 60_000,
            dimension: sailry_protocol::usage::Dimension::Model,
            utc_offset_minutes: 0,
            projects: session.project.into_iter().collect(),
            worktrees: vec![session.worktree],
            providers: vec![provider.id],
            models: vec![native::MODEL.into()],
            before: None,
        }),
    )
    .await
    else {
        panic!("usage report expected")
    };
    report
}

async fn sources(node: &Node, session: SessionId) -> Vec<Value> {
    node.with_storage(move |db| {
        Box::pin(async move {
            let mut statement = db
                .prepare(
                    "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence
                     WHERE h.session=?1 AND json_extract(e.body,'$.metadata.execution.purpose')='search'
                     ORDER BY h.sequence",
                )
                .await
                .map_err(crate::database_fixture::fault)?;
            let mut rows = statement
                .query([session.to_string()])
                .await
                .map_err(crate::database_fixture::fault)?;
            let mut records = Vec::new();
            while let Some(row) = rows.next().await.map_err(crate::database_fixture::fault)? {
                let bytes: Vec<u8> = row.get(0).map_err(crate::database_fixture::fault)?;
                records.push(serde_json::from_slice(&bytes).unwrap());
            }
            Ok::<_, Fault>(records)
        })
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn bills_requests_once_in_the_calling_turn() {
    for remote in [false, true] {
        let name = plugin_tool("web-search", "web_search");
        let main = MainServer::turn_tools(vec![(
            name.clone(),
            json!({"query":native::QUERY,"limit":1}),
        )])
        .await;
        let fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &main.endpoint,
            &["web-search"],
        )
        .await;
        let http = native::http::Server::start().await;
        let source = native::origin::Server::start().await;
        fixture
            .node
            .set_web_fixture(native::origin::HOST, source.address);
        let url = format!("{}/html", source.origin);
        http.respond("/v1/responses", vec![native::streamed(&url)]);
        let provider = native::provider(&fixture.client, &format!("{}/v1", http.endpoint)).await;
        assert_ne!(provider.id, fixture.provider.id);
        native::select(&fixture.client, &provider, true).await;
        let mut turns = Vec::new();
        for round in 1..=2 {
            let turn = fixture.submit("Find a source with native search").await;
            turns.push(turn);
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{:?}",
                page.runs
            );
            let results: Vec<_> = tool_details(&fixture.client, &page)
                .await
                .into_iter()
                .filter_map(|(tool, detail)| (tool == name).then(|| detail.result.unwrap()))
                .collect();
            assert_eq!(results.len(), round);
            for result in results {
                let page: search::Page = serde_json::from_value(result).unwrap();
                native::page(&page, &url);
            }
            assert_eq!(http.count("/v1/responses"), 1);
            let statistics =
                crate::observations::statistics(&fixture.client, fixture.session.id).await;
            assert_eq!(statistics.responses, round as u64 * 2 + 1);
            assert_eq!(
                statistics.usage,
                Some(Usage {
                    input: round as u64 * 24 + 31,
                    output: round as u64 * 8 + 9,
                    cached_input: 5,
                    reasoning: 2,
                })
            );
            assert_eq!(statistics.context_tokens, Some(16));
            assert_eq!(statistics.context_model.as_deref(), Some("fixture-a"));
            let report = report(&fixture.client, &fixture.session, &provider).await;
            assert_eq!(report.totals.responses, 1);
            assert_eq!(
                report.totals.tokens,
                Some(Usage {
                    input: 31,
                    output: 9,
                    cached_input: 5,
                    reasoning: 2,
                })
            );
            assert_eq!(report.requests.items.len(), 1);
            let call = &report.requests.items[0];
            assert_eq!(call.turn, turns[0]);
            assert_eq!(call.session, fixture.session.id);
            assert_eq!(call.provider, provider.id);
            assert_eq!(call.provider_name, native::NAME);
            assert_eq!(call.model, native::MODEL);
            let records = sources(&fixture.node, fixture.session.id).await;
            assert_eq!(records.len(), 1);
            let record = &records[0];
            assert_eq!(record["turn"], json!(turns[0]));
            assert_eq!(record["model"], native::MODEL);
            assert_eq!(
                record["usage"],
                json!({"input":31,"output":9,"cache_read":5,"reasoning":2})
            );
            assert_eq!(record["metadata"]["runtime"]["user_visible"], false);
            assert_eq!(record["metadata"]["runtime"]["agent_visible"], false);
            assert_eq!(
                record["metadata"]["execution"]["source"],
                json!({
                    "id":provider.id,"name":native::NAME,"model":native::MODEL,
                    "api":"responses","authentication":"api_key"
                })
            );
            assert!(!record.to_string().contains(native::KEY));
            assert!(
                page.entries
                    .iter()
                    .all(|entry| json!(entry.id) != record["id"])
            );
            {
                let requests = main.requests.lock().unwrap();
                assert_eq!(requests.len(), round * 2);
                for body in requests.iter() {
                    let tools = body["tools"].as_array().unwrap();
                    assert!(tools.iter().all(|tool| tool["type"] == "function"));
                    assert!(tools.iter().any(|tool| {
                        tool["function"]["name"]
                            .as_str()
                            .is_some_and(|name| name.contains("web_search"))
                    }));
                    assert!(!body.to_string().contains(record["id"].as_str().unwrap()));
                    assert!(!body.to_string().contains(native::KEY));
                }
            }
        }
        {
            let requests = http.requests.lock().unwrap();
            native::request(&requests[0].0, &requests[0].1, native::QUERY);
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
