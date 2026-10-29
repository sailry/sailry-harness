use super::*;
use sailry_protocol::web;

#[path = "../support/web.rs"]
mod origin;

async fn configure(client: &Client, enabled: bool) {
    let mut settings = read(client).await;
    settings
        .providers
        .get_mut(&Driver::Tinyfish)
        .unwrap()
        .enabled = enabled;
    save(
        client,
        settings,
        BTreeMap::from([(
            Driver::Tinyfish,
            search::Credentials {
                key: SecretUpdate::Replace(Secret::new("render-fixture-key".into())),
                ..Default::default()
            },
        )]),
    )
    .await;
}

async fn page(client: &Client, url: &str, offset: usize) -> web::Page {
    let Output::WebPage(page) = client
        .execute(client.prepare(Command::FetchWeb(web::Request {
            url: url.into(),
            offset,
        })))
        .await
        .unwrap()
    else {
        panic!("web page expected")
    };
    page
}

fn rendered(url: &str, final_url: &str) -> Reply {
    Reply::Json(serde_json::json!({
        "results":[{"url":url,"final_url":final_url,"title":"Rendered page","text":origin::paged()}],
        "errors":[]
    }))
}

#[tokio::test]
async fn shares_rendered_snapshots_across_service_paths() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let source = origin::Server::start().await;
        let renderer = Server::start().await;
        node.set_web_fixture(origin::HOST, source.address);
        node.set_search_fixture(Driver::Tinyfish, &format!("{}/render", renderer.endpoint));
        configure(&client, true).await;
        let url = format!("{}/empty-html", source.origin);
        let final_url = format!("{}/rendered", source.origin);
        renderer.respond("/render", vec![rendered(&url, &final_url)]);

        let mut body = String::new();
        let mut offset = 0;
        let mut pages = 0;
        loop {
            let page = page(&client, &url, offset).await;
            assert!(page.untrusted);
            assert_eq!(page.url, final_url);
            assert_eq!(page.title.as_deref(), Some("Rendered page"));
            assert_eq!(page.content_type, "text/markdown");
            assert_eq!(page.status, 200);
            assert!(page.content.chars().count() <= web::PAGE_CHARS);
            assert!(
                !serde_json::to_string(&page)
                    .unwrap()
                    .contains("render-fixture-key")
            );
            body.push_str(&page.content);
            pages += 1;
            let Some(next) = page.next_offset else { break };
            assert!(next > offset);
            offset = next;
        }
        assert_eq!(body, origin::paged());
        assert_eq!(pages, 2);
        configure(&client, false).await;
        let repeated = page(&client, &url, 0).await;
        assert_eq!(repeated.content.chars().count(), web::PAGE_CHARS);
        assert_eq!(source.requests.lock().unwrap().len(), 1);
        let requests = renderer.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let (header, request) = &requests[0];
        assert!(header.starts_with("POST /render "));
        assert!(
            header
                .to_ascii_lowercase()
                .contains("x-api-key: render-fixture-key")
        );
        assert_eq!(request["urls"], serde_json::json!([url]));
        assert_eq!(request["format"], "markdown");
        assert_eq!(request["ttl"], 0);
        assert!(
            (1..=web::TIMEOUT_SECS * 1000)
                .contains(&request["per_url_timeout_ms"].as_u64().unwrap())
        );
        assert!(!request.to_string().contains("render-fixture-key"));
        drop(requests);
        for header in source.requests.lock().unwrap().iter() {
            let lower = header.to_ascii_lowercase();
            assert!(!lower.contains("authorization:"));
            assert!(!lower.contains("x-api-key:"));
            assert!(!lower.contains("cookie:"));
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn skips_non_html_errors_and_sufficient_content() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let source = origin::Server::start().await;
        let renderer = Server::start().await;
        node.set_web_fixture(origin::HOST, source.address);
        node.set_search_fixture(Driver::Tinyfish, &format!("{}/render", renderer.endpoint));
        configure(&client, true).await;
        for path in ["/long-html", "/json", "/missing-html"] {
            let page = page(&client, &format!("{}{path}", source.origin), 0).await;
            assert!(page.untrusted);
            assert_ne!(page.content_type, "text/markdown");
        }
        configure(&client, false).await;
        let direct = page(&client, &format!("{}/html", source.origin), 0).await;
        assert_eq!(direct.content, origin::TEXT);
        assert!(renderer.requests.lock().unwrap().is_empty());
        assert_eq!(source.requests.lock().unwrap().len(), 4);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn reports_render_errors_without_caching_success() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let source = origin::Server::start().await;
        let renderer = Server::start().await;
        node.set_web_fixture(origin::HOST, source.address);
        node.set_search_fixture(Driver::Tinyfish, &format!("{}/render", renderer.endpoint));
        configure(&client, true).await;
        let url = format!("{}/html", source.origin);
        renderer.respond("/render", vec![
            Reply::Json(serde_json::json!({"results":[],"errors":[{"url":url,"error":"timeout","message":"render-fixture-key"}]})),
            rendered(&url, &url),
        ]);
        let error = client
            .execute(client.prepare(Command::FetchWeb(web::Request {
                url: url.clone(),
                offset: 0,
            })))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(!error.message.contains("render-fixture-key"));
        let recovered = page(&client, &url, 0).await;
        assert_eq!(recovered.title.as_deref(), Some("Rendered page"));
        assert_eq!(source.requests.lock().unwrap().len(), 2);
        assert_eq!(renderer.requests.lock().unwrap().len(), 2);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn validates_final_addresses_before_caching() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let source = origin::Server::start().await;
        let renderer = Server::start().await;
        node.set_web_fixture(origin::HOST, source.address);
        node.set_web_fixture("private.sailry.test", source.address);
        node.set_web_dns_fixture("private.sailry.test", vec!["127.0.0.1".parse().unwrap()]);
        node.set_search_fixture(Driver::Tinyfish, &format!("{}/render", renderer.endpoint));
        configure(&client, true).await;
        let url = format!("{}/html", source.origin);
        let final_url = format!("http://private.sailry.test:{}/data", source.address.port());
        renderer.respond("/render", vec![rendered(&url, &final_url)]);
        let error = client
            .execute(client.prepare(Command::FetchWeb(web::Request {
                url: url.clone(),
                offset: 0,
            })))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        let missing = client
            .execute(client.prepare(Command::FetchWeb(web::Request {
                url,
                offset: web::PAGE_CHARS,
            })))
            .await
            .unwrap_err();
        assert_eq!(missing.code, ErrorCode::NotFound);
        assert_eq!(source.requests.lock().unwrap().len(), 1);
        assert_eq!(renderer.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn cancels_render_requests_with_the_fetch() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let (controller, transport) = controller(&node, directory.path()).await;
        let client = Client::new(if remote { transport } else { node.local() });
        let source = origin::Server::start().await;
        let renderer = Server::start().await;
        node.set_web_fixture(origin::HOST, source.address);
        node.set_search_fixture(Driver::Tinyfish, &format!("{}/render", renderer.endpoint));
        configure(&client, true).await;
        renderer.holds.lock().unwrap().insert("/render".into());
        let request = client.prepare(Command::FetchWeb(web::Request {
            url: format!("{}/html", source.origin),
            offset: 0,
        }));
        let id = request.id;
        let pending = client.dispatch(request).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), renderer.held.cancelled())
            .await
            .unwrap();
        client
            .execute(client.prepare(Command::CancelWebFetch { request: id }))
            .await
            .unwrap();
        let error = tokio::time::timeout(Duration::from_secs(5), pending.completion)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Cancelled);
        tokio::time::timeout(Duration::from_secs(5), renderer.disconnected.cancelled())
            .await
            .unwrap();
        assert_eq!(source.requests.lock().unwrap().len(), 1);
        assert_eq!(renderer.requests.lock().unwrap().len(), 1);
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
