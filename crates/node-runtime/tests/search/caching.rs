use super::*;

async fn configured(client: &Client) -> search::Settings {
    let mut settings = read(client).await;
    settings.driver = Some(Selection::Driver(Driver::Tavily));
    settings.fallback = false;
    for (driver, config) in &mut settings.providers {
        config.enabled = *driver == Driver::Tavily;
    }
    save(
        client,
        settings,
        BTreeMap::from([(
            Driver::Tavily,
            search::Credentials {
                key: SecretUpdate::Replace(Secret::new("cache-fixture-key".into())),
                ..Default::default()
            },
        )]),
    )
    .await
}

async fn page(client: &Client, input: search::Request) -> search::Page {
    let Output::SearchPage(page) = client
        .execute(client.prepare(Command::SearchWeb(input)))
        .await
        .unwrap()
    else {
        panic!("search page expected")
    };
    page
}

#[tokio::test]
async fn shares_hits_and_settings_invalidation() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = Server::start().await;
        node.set_search_fixture(Driver::Tavily, &http.url(Driver::Tavily));
        let mut settings = configured(&client).await;
        let original = search::Request {
            query: " source ".into(),
            include_domains: vec![
                "DOCS.example.org".into(),
                "EXAMPLE.COM.".into(),
                "example.com".into(),
            ],
            exclude_domains: vec!["PRIVATE.example.com".into()],
            ..Default::default()
        };
        let first = page(&client, original.clone()).await;
        assert!(first.untrusted);
        assert_eq!(first.results.len(), 1);
        assert_eq!(page(&client, original.normalized()).await, first);
        assert_eq!(http.count("/tavily"), 1);
        settings = save(
            &client,
            settings,
            BTreeMap::from([(
                Driver::Tavily,
                search::Credentials {
                    key: SecretUpdate::Replace(Secret::new("updated-cache-key".into())),
                    ..Default::default()
                },
            )]),
        )
        .await;
        assert_eq!(page(&client, original.clone()).await, first);
        assert_eq!(http.count("/tavily"), 2);
        settings.driver = Some(Selection::Auto);
        save(&client, settings, Default::default()).await;
        assert_eq!(page(&client, original).await, first);
        assert_eq!(http.count("/tavily"), 3);
        for (header, _) in http.requests.lock().unwrap().iter() {
            assert!(header.starts_with("POST /tavily "));
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn driver_tests_always_reach_the_service() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = Server::start().await;
        node.set_search_fixture(Driver::Tavily, &http.url(Driver::Tavily));
        configured(&client).await;
        let input = search::Request {
            query: "Sailry".into(),
            limit: 1,
            ..Default::default()
        };
        let first = page(&client, input.clone()).await;
        assert_eq!(page(&client, input.clone()).await, first);
        assert_eq!(http.count("/tavily"), 1);
        for count in 2..=3 {
            let Output::SearchPage(tested) = client
                .execute(client.prepare(Command::TestSearchDriver {
                    driver: Driver::Tavily,
                }))
                .await
                .unwrap()
            else {
                panic!("driver test page expected")
            };
            assert_eq!(tested, first);
            assert_eq!(http.count("/tavily"), count);
        }
        http.respond(
            "/tavily",
            vec![Reply::Json(serde_json::json!({"bad":"response"}))],
        );
        let error = client
            .execute(client.prepare(Command::TestSearchDriver {
                driver: Driver::Tavily,
            }))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(http.count("/tavily"), 4);
        assert_eq!(page(&client, input).await, first);
        assert_eq!(http.count("/tavily"), 4);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
