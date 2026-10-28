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
                    cached_input: 6,
                    reasoning: 0,
                }]
            );
            let profile = fixture.node.profile().to_owned();
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            let (database, db) =
                crate::database_fixture::open(&profile.join("storage/node.turso"), true).await;
            let body: Vec<u8> = db
                .prepare("SELECT body FROM agent_events WHERE json_type(body,'$.usage')='object'")
                .await
                .unwrap()
                .query_row(())
                .await
                .unwrap()
                .get(0)
                .unwrap();
            let record: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(record["model"], "fixture-a");
            let usage = &record["usage"];
            assert_eq!(usage["input"], 16);
            assert_eq!(usage["output"], 4);
            assert_eq!(usage["cache_read"], 6);
            if api == ModelApi::Anthropic {
                assert_eq!(usage["cache_write"], 2);
            }
            assert_eq!(usage["reasoning"], serde_json::Value::Null);
            assert!(record["metadata"]["execution"]["timing"]["elapsed_us"].is_u64());
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
