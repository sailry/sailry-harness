use super::*;

#[tokio::test]
async fn preserves_metadata_and_answer() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::Gemini, Reply::Grounding).await;
        let fixture =
            Fixture::packages(remote, ModelApi::Gemini, &server.endpoint, &["web-search"]).await;
        let mut provider = fixture.provider.clone();
        provider.models[0].web_search = true;
        execute(
            &fixture.client,
            Command::PutProvider {
                expected_revision: provider.revision,
                provider,
            },
        )
        .await;
        let turn = fixture.submit("Keep the provider response").await;
        let page = finished(&fixture.client, fixture.session.id, turn).await;
        assert_eq!(page.runs[0].status, Status::Completed);
        let texts: Vec<_> = page
            .entries
            .iter()
            .flat_map(|entry| &entry.parts)
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["Keep the provider response", server::ANSWER]);
        let entry = page
            .entries
            .iter()
            .find(|entry| entry.search_suggestions.is_some())
            .unwrap();
        assert_eq!(
            entry.search_suggestions.as_deref(),
            server::grounding_metadata()["searchEntryPoint"]["renderedContent"].as_str()
        );
        assert_eq!(entry.citations.len(), 1);
        assert_eq!(entry.citations[0].uri, "https://example.com/source");
        assert_eq!(entry.citations[0].start, None);
        let public = serde_json::to_string(&page).unwrap();
        for private in ["sdkBlob", "provider_origin", server::KEY] {
            assert!(!public.contains(private));
        }
        {
            let requests = server.requests.lock().unwrap();
            let tools = requests[0].body["tools"].as_array().unwrap();
            assert!(tools.iter().any(|tool| tool["google_search"].is_object()));
        }
        let profile = fixture.node.profile().to_owned();
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
        let (database, db) =
            crate::database_fixture::open(&profile.join("storage/node.turso"), true).await;
        let native: String = db.prepare(
            "SELECT json_extract(body,'$.provider_metadata') FROM agent_events WHERE json_type(body,'$.provider_metadata.searchEntryPoint')='object'",
        ).await.unwrap().query_row(()).await.unwrap().get(0).unwrap();
        let mut native: serde_json::Value = serde_json::from_str(&native).unwrap();
        assert_eq!(
            native.as_object_mut().unwrap().remove("content_complete"),
            Some(serde_json::Value::Bool(true))
        );
        let timing = native
            .as_object_mut()
            .unwrap()
            .remove("sailry_timing")
            .unwrap();
        assert!(timing["elapsed_us"].as_u64().unwrap() > 0);
        let origin: adk_core::ProviderOrigin =
            serde_json::from_value(native["provider_origin"].clone()).unwrap();
        assert_eq!(origin.provider, "gemini");
        assert_eq!(origin.model, "fixture-a");
        assert_eq!(origin.scope.len(), 64);
        assert!(
            origin
                .scope
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        let public_origin = serde_json::to_string(&origin).unwrap();
        let credential = fixture
            .session
            .config
            .credential
            .as_ref()
            .unwrap()
            .id
            .to_string();
        for private in [server::KEY, server.endpoint.as_str(), credential.as_str()] {
            assert!(!public_origin.contains(private));
        }
        let mut expected = server::grounding_metadata();
        expected["provider_origin"] = serde_json::json!({
            "provider":"gemini", "model":"fixture-a", "scope":origin.scope,
        });
        assert_eq!(native, expected);
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
