use super::*;

#[tokio::test]
async fn normalizes_cache_and_thoughts() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Usage).await;
            let fixture = Fixture::empty(remote, api, &server.endpoint).await;
            let turn = fixture.submit("Count provider usage").await;
            let page = finished(&fixture.client, fixture.session.id, turn).await;
            assert_eq!(page.runs[0].status, Status::Completed);
            let reported: Vec<_> = page
                .entries
                .iter()
                .filter_map(|entry| entry.usage.clone())
                .collect();
            assert_eq!(
                reported,
                vec![Usage {
                    input: 16,
                    output: 4,
                    cached_input: if api == ModelApi::DeepSeek { 0 } else { 6 },
                    reasoning: 0,
                }]
            );
            let profile = fixture.node.profile().to_owned();
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            let (database, db) =
                crate::database_fixture::open(&profile.join("storage/node.turso"), true).await;
            let native: String = db.prepare(
                "SELECT json_extract(body,'$.usage.provider') FROM agent_events WHERE json_type(body,'$.usage.provider')='object'",
            ).await.unwrap().query_row(()).await.unwrap().get(0).unwrap();
            let native: serde_json::Value = serde_json::from_str(&native).unwrap();
            assert_eq!(native["model"], "fixture-a");
            assert_eq!(native["usage"]["input_tokens"], 16);
            assert_eq!(native["usage"]["output_tokens"], 4);
            assert_eq!(
                native["usage"]["cache_read_input_tokens"],
                if api == ModelApi::DeepSeek {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(6)
                }
            );
            if api == ModelApi::Anthropic {
                assert_eq!(native["usage"]["cache_write_input_tokens"], 2);
            }
            drop(db);
            drop(database);
            let node = Node::start_empty(profile).await.unwrap();
            assert_eq!(
                history(&Client::new(node.local()), fixture.session.id).await,
                page
            );
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            node.shutdown().await.unwrap();
        }
    }
}
