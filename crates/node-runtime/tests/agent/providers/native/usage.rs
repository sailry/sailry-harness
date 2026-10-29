use super::*;
use serde_json::json;

fn attempt(input: u64, output: u64, text: Option<&str>) -> server::Script {
    let content: Vec<_> = text
        .map(|text| {
            json!({"type":"message", "id":"answer", "role":"assistant", "status":"completed",
                "content":[{"type":"output_text", "text":text, "annotations":[]}]})
        })
        .into_iter()
        .collect();
    let event = json!({"type":"response.completed", "sequence_number":1,
        "response":{"id":format!("usage-{input}"), "object":"response", "created_at":1,
            "model":"fixture-a", "status":"completed", "output":content,
            "usage":{"input_tokens":input,"output_tokens":output,"total_tokens":input+output}}});
    server::Script {
        bytes: format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {event}\n\ndata: [DONE]\n\n"
        )
        .into_bytes(),
        ending: server::Ending::Close,
    }
}

#[tokio::test]
async fn retains_empty_attempt_before_success() {
    for remote in [false, true] {
        let server = Server::start(
            ModelApi::Responses,
            Reply::Scripted(Arc::new(vec![
                attempt(12, 3, None),
                attempt(20, 5, Some(server::ANSWER)),
            ])),
        )
        .await;
        let fixture = Fixture::empty(remote, ModelApi::Responses, &server.endpoint).await;
        let turn = fixture.submit("Count every inference attempt").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        assert_eq!(completion::response(&page), server::ANSWER);
        let reported: Vec<_> = page
            .entries
            .iter()
            .filter_map(|entry| entry.usage.clone())
            .collect();
        assert_eq!(
            reported,
            vec![Usage {
                input: 20,
                output: 5,
                cached_input: 0,
                reasoning: 0,
            }]
        );
        let statistics = crate::observations::statistics(&fixture.client, fixture.session.id).await;
        assert_eq!(statistics.responses, 2);
        assert_eq!(
            statistics.usage,
            Some(Usage {
                input: 32,
                output: 8,
                cached_input: 0,
                reasoning: 0,
            })
        );
        assert_eq!(statistics.context_tokens, Some(25));
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        let profile = fixture.node.profile().to_owned();
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        let (database, db) =
            crate::database_fixture::open(&profile.join("storage/node.turso"), true).await;
        let mut query = db
            .prepare("SELECT body FROM agent_events WHERE json_type(body,'$.usage')='object' ORDER BY sequence")
            .await
            .unwrap();
        let mut rows = query.query(()).await.unwrap();
        let mut records = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            let body: Vec<u8> = row.get(0).unwrap();
            records.push(serde_json::from_slice::<serde_json::Value>(&body).unwrap());
        }
        assert_eq!(records.len(), 2);
        assert_ne!(records[0]["id"], records[1]["id"]);
        assert_eq!(records[0]["usage"]["input"], 12);
        assert_eq!(records[0]["usage"]["output"], 3);
        assert_eq!(records[0]["metadata"]["runtime"]["user_visible"], false);
        assert_eq!(records[0]["metadata"]["runtime"]["agent_visible"], false);
        assert_eq!(records[1]["usage"]["input"], 20);
        assert_eq!(records[1]["usage"]["output"], 5);
        drop(rows);
        drop(query);
        drop(db);
        drop(database);
        let node = Node::start_empty(profile).await.unwrap();
        let client = Client::new(node.local());
        assert_eq!(history(&client, fixture.session.id).await, page);
        assert_eq!(
            crate::observations::statistics(&client, fixture.session.id).await,
            statistics
        );
        assert_eq!(server.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
    }
}

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
