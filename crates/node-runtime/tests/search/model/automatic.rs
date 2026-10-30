use super::*;

fn unsupported() -> native::http::Reply {
    native::http::Reply::Json(serde_json::json!({"error":{"code":"unsupported_tool"}}))
}

#[tokio::test]
async fn follows_order_and_invalidates_cached_pages() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = native::http::Server::start().await;
        let fallback = native::http::Server::start().await;
        let source = native::origin::Server::start().await;
        node.set_web_fixture(native::origin::HOST, source.address);
        node.set_search_fixture(Driver::Exa, &fallback.url(Driver::Exa));
        let url = format!("{}/html", source.origin);
        http.respond(
            "/v1/responses",
            vec![native::response(&url), native::response(&url)],
        );
        let provider = native::provider(&client, &format!("{}/v1", http.endpoint)).await;
        let mut settings = native::select(&client, &provider, true).await;
        settings.driver = Some(Selection::Auto);
        settings
            .order
            .sort_by_key(|driver| *driver != Driver::Model);
        settings = save(&client, settings, Default::default()).await;
        native::page(&page(&client, native::QUERY).await.unwrap(), &url);
        native::page(&page(&client, native::QUERY).await.unwrap(), &url);
        assert_eq!(http.count("/v1/responses"), 1);
        assert_eq!(fallback.count("/exa"), 0);
        settings.order.sort_by_key(|driver| *driver != Driver::Exa);
        settings = save(&client, settings, Default::default()).await;
        assert_eq!(
            page(&client, native::QUERY).await.unwrap().driver,
            Driver::Exa
        );
        assert_eq!(http.count("/v1/responses"), 1);
        assert_eq!(fallback.count("/exa"), 1);
        settings.providers.get_mut(&Driver::Exa).unwrap().enabled = false;
        save(&client, settings, Default::default()).await;
        native::page(&page(&client, native::QUERY).await.unwrap(), &url);
        assert_eq!(http.count("/v1/responses"), 2);
        assert_eq!(fallback.count("/exa"), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn falls_back_on_recoverable_model_failures() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let http = native::http::Server::start().await;
        let fallback = native::http::Server::start().await;
        node.set_search_fixture(Driver::Exa, &fallback.url(Driver::Exa));
        http.respond(
            "/v1/responses",
            vec![
                unsupported(),
                native::http::Reply::Status(401, None),
                native::http::Reply::Status(503, None),
                unsupported(),
                native::http::Reply::Json(serde_json::json!({"invalid":"response"})),
            ],
        );
        let provider = native::provider(&client, &format!("{}/v1", http.endpoint)).await;
        let mut settings = native::select(&client, &provider, true).await;
        settings.driver = Some(Selection::Auto);
        settings
            .order
            .sort_by_key(|driver| *driver != Driver::Model);
        settings = save(&client, settings, Default::default()).await;
        for index in 0..3 {
            assert_eq!(
                page(&client, &format!("Search {index}"))
                    .await
                    .unwrap()
                    .driver,
                Driver::Exa
            );
        }
        assert_eq!(http.count("/v1/responses"), 3);
        assert_eq!(fallback.count("/exa"), 3);
        settings.fallback = false;
        settings = save(&client, settings, Default::default()).await;
        assert_eq!(
            page(&client, "No fallback").await.unwrap_err().details,
            Some(FaultDetails::SearchUnsupported)
        );
        assert_eq!(fallback.count("/exa"), 3);
        settings.fallback = true;
        save(&client, settings, Default::default()).await;
        assert_eq!(
            page(&client, "Invalid response").await.unwrap_err().code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(fallback.count("/exa"), 3);
        assert_eq!(http.count("/v1/responses"), 5);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
