use super::*;
use sailry_protocol::usage::{Cost, CostBreakdown};

async fn price(node: &Node, supplier: &'static str) {
    node.with_storage(move |db| {
        Box::pin(async move {
            db.execute(
                "INSERT INTO model_catalog(provider,id,body) VALUES(?1,?2,?3)",
                turso::params![
                    supplier,
                    native::MODEL,
                    serde_json::to_vec(&json!({
                        "id":native::MODEL,
                        "cost":{"input":2,"output":4,"cache_read":1,"cache_write":3}
                    }))
                    .unwrap()
                ],
            )
            .await
            .map_err(crate::database_fixture::fault)?;
            Ok::<_, Fault>(())
        })
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn retains_usage_while_cancelling_source_reads() {
    for remote in [false, true] {
        let name = plugin_tool("web-search", "web_search");
        let main =
            MainServer::turn_tools(vec![(name, json!({"query":native::QUERY,"limit":1}))]).await;
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
        fixture
            .node
            .set_search_fixture(search::Driver::Exa, &http.url(search::Driver::Exa));
        let url = format!("{}/hold", source.origin);
        http.respond("/v1/responses", vec![native::response(&url)]);
        let provider = native::provider(&fixture.client, &format!("{}/v1", http.endpoint)).await;
        let mut settings = native::select(&fixture.client, &provider, true).await;
        settings.driver = Some(search::Selection::Auto);
        settings
            .order
            .sort_by_key(|driver| *driver != search::Driver::Model);
        execute(
            &fixture.client,
            Command::SaveSearchSettings(search::Update {
                settings,
                credentials: Default::default(),
            }),
        )
        .await;
        price(&fixture.node, "openai").await;

        let turn = fixture.submit("Search and cancel the source read").await;
        tokio::time::timeout(Duration::from_secs(10), source.held.cancelled())
            .await
            .unwrap();
        execute(&fixture.client, Command::StopTurn { turn }).await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Cancelled);
        tokio::time::timeout(Duration::from_secs(5), source.disconnected.cancelled())
            .await
            .unwrap();

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
        assert_eq!(
            report.totals.cost,
            Some(Cost {
                usd_micros: 93,
                responses: 1,
                breakdown: Some(CostBreakdown {
                    input: 52,
                    output: 36,
                    cache_read: 5,
                    cache_write: 0,
                }),
            })
        );
        assert_eq!(report.requests.items.len(), 1);
        let call = &report.requests.items[0];
        assert_eq!(call.session, fixture.session.id);
        assert_eq!(call.turn, turn);
        assert_eq!(call.provider, provider.id);
        assert_eq!(call.provider_name, native::NAME);
        assert_eq!(call.model, native::MODEL);
        assert_eq!(call.usd_micros, Some(93));
        let records = sources(&fixture.node, fixture.session.id).await;
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record["turn"], json!(turn));
        assert_eq!(record["model"], native::MODEL);
        assert_eq!(record["usage"]["input"], 31);
        assert_eq!(record["usage"]["output"], 9);
        assert_eq!(record["usage"]["cache_read"], 5);
        assert_eq!(record["usage"]["reasoning"], 2);
        assert_eq!(record["metadata"]["runtime"]["user_visible"], false);
        assert_eq!(record["metadata"]["runtime"]["agent_visible"], false);
        assert_eq!(
            record["metadata"]["execution"]["source"]["id"],
            json!(provider.id)
        );
        assert!(!record.to_string().contains(native::KEY));
        assert!(
            page.entries
                .iter()
                .all(|entry| json!(entry.id) != record["id"])
        );
        assert_eq!(main.requests.lock().unwrap().len(), 1);
        assert_eq!(http.count("/v1/responses"), 1);
        assert_eq!(http.count("/exa"), 0);
        assert_eq!(http.requests.lock().unwrap().len(), 1);
        assert_eq!(source.requests.lock().unwrap().len(), 1);
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn retains_paused_usage_after_continuation_failure() {
    for remote in [false, true] {
        let name = plugin_tool("web-search", "web_search");
        let main =
            MainServer::turn_tools(vec![(name, json!({"query":native::QUERY,"limit":1}))]).await;
        let fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &main.endpoint,
            &["web-search"],
        )
        .await;
        let http = native::http::Server::start().await;
        fixture
            .node
            .set_search_fixture(search::Driver::Exa, &http.url(search::Driver::Exa));
        let mut provider =
            native::provider(&fixture.client, &format!("{}/v1", http.endpoint)).await;
        provider.api = ModelApi::Anthropic;
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let mut settings = native::select(&fixture.client, &provider, true).await;
        settings.driver = Some(search::Selection::Auto);
        settings
            .order
            .sort_by_key(|driver| *driver != search::Driver::Model);
        execute(
            &fixture.client,
            Command::SaveSearchSettings(search::Update {
                settings,
                credentials: Default::default(),
            }),
        )
        .await;
        price(&fixture.node, "anthropic").await;
        let paused = json!({
            "type":"message","role":"assistant","model":native::MODEL,
            "stop_reason":"pause_turn",
            "content":[
                {"type":"server_tool_use","id":"paused-search","name":"web_search",
                    "input":{"query":native::QUERY}},
                {"type":"web_search_tool_result","tool_use_id":"paused-search","content":[]}
            ],
            "usage":{"input_tokens":10,"output_tokens":3,
                "cache_read_input_tokens":2,"cache_creation_input_tokens":1}
        });
        http.respond(
            "/v1/messages",
            vec![
                native::http::Reply::Json(paused.clone()),
                native::http::Reply::Status(503, None),
            ],
        );

        let turn = fixture.submit("Search through a failed continuation").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs.last().unwrap().status, Status::Completed);
        let report = report(&fixture.client, &fixture.session, &provider).await;
        assert_eq!(report.totals.responses, 1);
        assert_eq!(
            report.totals.tokens,
            Some(Usage {
                input: 13,
                output: 3,
                cached_input: 2,
                reasoning: 0,
            })
        );
        assert_eq!(
            report.totals.cost,
            Some(Cost {
                usd_micros: 37,
                responses: 1,
                breakdown: Some(CostBreakdown {
                    input: 20,
                    output: 12,
                    cache_read: 2,
                    cache_write: 3,
                }),
            })
        );
        assert_eq!(report.requests.items.len(), 1);
        let call = &report.requests.items[0];
        assert_eq!(call.session, fixture.session.id);
        assert_eq!(call.turn, turn);
        assert_eq!(call.provider, provider.id);
        assert_eq!(call.model, native::MODEL);
        assert_eq!(call.usd_micros, Some(37));
        let records = sources(&fixture.node, fixture.session.id).await;
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record["turn"], json!(turn));
        assert_eq!(record["model"], native::MODEL);
        assert_eq!(record["usage"]["input"], 13);
        assert_eq!(record["usage"]["cache_read"], 2);
        assert_eq!(record["usage"]["cache_write"], 1);
        assert_eq!(record["usage"]["output"], 3);
        assert_eq!(record["metadata"]["runtime"]["user_visible"], false);
        assert_eq!(record["metadata"]["runtime"]["agent_visible"], false);
        assert_eq!(
            record["metadata"]["execution"]["source"]["api"],
            "anthropic"
        );
        assert!(!record.to_string().contains(native::KEY));
        assert!(
            page.entries
                .iter()
                .all(|entry| json!(entry.id) != record["id"])
        );
        assert_eq!(http.count("/v1/messages"), 2);
        assert_eq!(http.count("/exa"), 1);
        let (_, detail) = tool_details(&fixture.client, &page)
            .await
            .into_iter()
            .find(|(tool, _)| tool == &plugin_tool("web-search", "web_search"))
            .unwrap();
        let result: search::Page = serde_json::from_value(detail.result.unwrap()).unwrap();
        assert_eq!(result.driver, search::Driver::Exa);
        assert_eq!(main.requests.lock().unwrap().len(), 2);
        {
            let requests = http.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert_eq!(requests[1].1["messages"][1]["role"], "assistant");
            assert_eq!(requests[1].1["messages"][1]["content"], paused["content"]);
            assert!(requests[..2].iter().all(|(header, body)| {
                header
                    .to_ascii_lowercase()
                    .contains(&format!("x-api-key: {}", native::KEY))
                    && !body.to_string().contains(native::KEY)
            }));
            assert!(!requests[2].0.contains(native::KEY));
            assert!(!requests[2].1.to_string().contains(native::KEY));
        }
        fixture.node.shutdown().await.unwrap();
        fixture.controller.close().await.unwrap();
    }
}
