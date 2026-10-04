use super::*;

#[tokio::test]
async fn normalizes_cache_and_thoughts() {
    for api in [ModelApi::Anthropic, ModelApi::Gemini, ModelApi::DeepSeek] {
        for remote in [false, true] {
            let server = Server::start(api, Reply::Usage).await;
            let fixture = Fixture::new(remote, api, &server.endpoint).await;
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
                    reasoning: u64::from(api != ModelApi::Anthropic),
                }]
            );
            let profile = fixture.node.profile().to_owned();
            fixture.controller.close().await.unwrap();
            fixture.node.shutdown().await.unwrap();
            let db = rusqlite::Connection::open_with_flags(
                profile.join("storage/node.sqlite3").canonicalize().unwrap(),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let native: String = db.query_row(
                "SELECT json_extract(body,'$.usage_metadata.provider_usage') FROM agent_events WHERE json_type(body,'$.usage_metadata')='object'",
                [], |row| row.get(0),
            ).unwrap();
            let native: serde_json::Value = serde_json::from_str(&native).unwrap();
            if api == ModelApi::Anthropic {
                assert_eq!(native["input_tokens"], 8);
                assert_eq!(native["cache_creation_input_tokens"], 2);
                assert_eq!(native["cache_read_input_tokens"], 6);
            } else if api == ModelApi::Gemini {
                assert_eq!(native["candidatesTokenCount"], 3);
                assert_eq!(native["thoughtsTokenCount"], 1);
            } else {
                assert_eq!(native["prompt_tokens"], 16);
                assert_eq!(native["prompt_cache_hit_tokens"], 6);
                assert_eq!(native["prompt_cache_miss_tokens"], 10);
                assert_eq!(native["completion_tokens_details"]["reasoning_tokens"], 1);
            }
            drop(db);
            let node = Node::start(profile).await.unwrap();
            assert_eq!(
                history(&Client::new(node.local()), fixture.session.id).await,
                page
            );
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            node.shutdown().await.unwrap();
        }
    }
}
