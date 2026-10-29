use super::*;
use sailry_protocol::search::Freshness;
use serde_json::json;

#[tokio::test]
async fn normalizes_requests_and_narrows_results() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = Server::start().await;
        node.set_search_fixture(Driver::Tavily, &http.url(Driver::Tavily));
        let mut settings = read(&client).await;
        settings.driver = Some(Selection::Driver(Driver::Tavily));
        settings.fallback = false;
        settings.providers.get_mut(&Driver::Tavily).unwrap().enabled = true;
        save(
            &client,
            settings,
            BTreeMap::from([(
                Driver::Tavily,
                search::Credentials {
                    key: SecretUpdate::Replace(Secret::new("synthetic-filter-key".into())),
                    ..Default::default()
                },
            )]),
        )
        .await;
        let now = chrono::Utc::now().to_rfc3339();
        http.respond(
            "/tavily",
            vec![Reply::Json(json!({"results":[
                {"title":"Current", "url":"https://example.com/current", "content":"Current content", "published_date":now},
                {"title":"Undated", "url":"https://docs.example.com/undated", "content":"Undated content"},
                {"title":"Old", "url":"https://example.com/old", "content":"Old content", "published_date":"2000-01-01"},
                {"title":"Outside", "url":"https://other.test/current", "content":"Outside content", "published_date":now},
                {"title":"Excluded", "url":"https://blocked.example.com/current", "content":"Excluded content", "published_date":now},
                {"title":"Other suffix", "url":"https://notexample.com/current", "content":"Other suffix content", "published_date":now}
            ]}))],
        );
        let input = search::Request {
            query: "  latest  Rust release  ".into(),
            limit: 5,
            freshness: Some(Freshness::Day),
            include_domains: vec!["EXAMPLE.com.".into(), "example.com".into()],
            exclude_domains: vec!["BLOCKED.example.com.".into()],
        };
        let Output::SearchPage(page) = client
            .execute(client.prepare(Command::SearchWeb(input.clone())))
            .await
            .unwrap()
        else {
            panic!("search page expected")
        };
        assert_eq!(page.driver, Driver::Tavily);
        assert!(page.untrusted);
        assert_eq!(
            page.results
                .iter()
                .map(|result| result.title.as_str())
                .collect::<Vec<_>>(),
            ["Current", "Undated"]
        );
        assert!(page.results[1].published.is_none());
        {
            let requests = http.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            let (_, body) = &requests[0];
            assert_eq!(body["query"], "latest  Rust release");
            assert_eq!(body["time_range"], "day");
            assert_eq!(body["include_domains"], json!(["example.com"]));
            assert_eq!(body["exclude_domains"], json!(["blocked.example.com"]));
        }
        for (include_domains, exclude_domains) in [
            (vec!["EXAMPLE.com.".into()], vec!["example.com".into()]),
            (vec!["https://example.com".into()], vec![]),
            (vec![], vec!["example.com".into(); search::MAX_DOMAINS + 1]),
        ] {
            let request = search::Request {
                include_domains,
                exclude_domains,
                ..input.clone()
            };
            assert_eq!(
                client
                    .execute(client.prepare(Command::SearchWeb(request)))
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
        }
        assert_eq!(http.count("/tavily"), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
