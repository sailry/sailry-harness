use super::*;
#[path = "../../../tests/support/search.rs"]
mod server;
use server::{Reply, Server};

impl Server {
    fn stall(&self, method: &str) {
        self.stalls.lock().unwrap().insert(method.into());
    }

    fn method_count(&self, method: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(header, _)| header.split_whitespace().next() == Some(method))
            .count()
    }
}

fn plan(drivers: &[Driver]) -> Plan {
    let mut plan = Plan::default();
    for driver in Driver::ALL {
        let config = plan.settings.providers.get_mut(&driver).unwrap();
        config.enabled = drivers.contains(&driver);
        if driver.key_required() || driver == Driver::Exa {
            config.key_configured = true;
            plan.credentials.insert(
                driver,
                Auth {
                    key: Some(Secret::new("fixture-key".into())),
                    basic: None,
                },
            );
        }
        if driver == Driver::Searxng {
            config.endpoint = Some("https://search.example/".into());
        }
    }
    plan.settings.driver = Some(Selection::Driver(drivers[0]));
    plan
}

fn input() -> search::Request {
    search::Request {
        query: "source 中文 🙂".into(),
        limit: 5,
    }
}

async fn search(owner: &Arc<Searcher>, plan: Plan) -> Result<search::Page, Fault> {
    owner
        .search(
            NodeId([1; 32]),
            RequestId::new(),
            plan,
            &input(),
            &CancellationToken::new(),
        )
        .await
}

#[tokio::test]
async fn maps_five_driver_responses_and_requests() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    for driver in Driver::ALL {
        let plan = plan(&[driver]);
        owner.configure(plan.settings.clone());
        owner.fixture(driver, &server.url(driver));
        let page = search(&owner, plan).await.unwrap();
        assert_eq!(page.driver, driver);
        assert!(page.untrusted);
        assert_eq!(page.results.len(), 1);
        assert_eq!(page.results[0].title, "Public source");
        assert_eq!(page.results[0].snippet, "Untrusted content");
        assert_eq!(page.results[0].published.as_deref(), Some("2026-10-09"));
    }
    let requests = server.requests.lock().unwrap();
    for (header, body) in requests.iter() {
        let lower = header.to_ascii_lowercase();
        if header.starts_with("GET /brave") {
            assert!(lower.contains("x-subscription-token: fixture-key"));
            assert!(header.contains("count=5"));
        } else if header.starts_with("GET /searxng") {
            assert!(header.contains("format=json"));
            assert!(!lower.contains("authorization:"));
        } else if header.starts_with("POST /exa") {
            assert!(lower.contains("x-api-key: fixture-key"));
            assert_eq!(body["numResults"], 5);
        } else {
            assert!(lower.contains("authorization: bearer fixture-key"));
            assert_eq!(body["query"], input().query);
        }
    }
}

#[tokio::test]
async fn parses_keyless_hosted_mcp_results() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    owner.fixture(Driver::Exa, &format!("{}/mcp", server.endpoint));
    let page = search(&owner, Plan::default()).await.unwrap();
    assert_eq!(page.results.len(), 2);
    assert!(page.results[1].published.is_none());
    let requests = server.requests.lock().unwrap();
    let calls: Vec<_> = requests
        .iter()
        .filter(|(_, body)| body["method"] == "tools/call")
        .collect();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1["params"]["name"], "web_search_exa");
    assert_eq!(calls[0].1["params"]["arguments"]["numResults"], 5);
    assert!(
        requests
            .iter()
            .all(|(header, _)| !header.to_ascii_lowercase().contains("authorization:"))
    );
}

#[tokio::test]
async fn cancelled_initialization_releases_connection() {
    let server = Server::start().await;
    server.stall("initialize");
    let owner = Arc::new(Searcher::default());
    owner.fixture(Driver::Exa, &format!("{}/mcp", server.endpoint));
    let caller = NodeId([2; 32]);
    let id = RequestId::new();
    let active = owner.clone();
    let pending = tokio::spawn(async move {
        active
            .search(
                caller,
                id,
                Plan::default(),
                &input(),
                &CancellationToken::new(),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), server.held.cancelled())
        .await
        .unwrap();
    owner.cancel(caller, id);
    let error = tokio::time::timeout(Duration::from_secs(10), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(
        error,
        Fault::new(ErrorCode::Cancelled, "web search was cancelled")
    );
    tokio::time::timeout(Duration::from_secs(5), server.disconnected.cancelled())
        .await
        .unwrap();
    assert_eq!(server.method_count("DELETE"), 0);
}

#[tokio::test]
async fn cancelled_tool_releases_session() {
    let server = Server::start().await;
    server.stall("tools/call");
    let owner = Arc::new(Searcher::default());
    owner.fixture(Driver::Exa, &format!("{}/mcp", server.endpoint));
    let caller = NodeId([3; 32]);
    let id = RequestId::new();
    let active = owner.clone();
    let pending = tokio::spawn(async move {
        active
            .search(
                caller,
                id,
                Plan::default(),
                &input(),
                &CancellationToken::new(),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), server.held.cancelled())
        .await
        .unwrap();
    owner.cancel(caller, id);
    let error = tokio::time::timeout(Duration::from_secs(10), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(
        error,
        Fault::new(ErrorCode::Cancelled, "web search was cancelled")
    );
    tokio::time::timeout(Duration::from_secs(5), server.disconnected.cancelled())
        .await
        .unwrap();
    assert_eq!(server.method_count("DELETE"), 1);
    assert!(server.requests.lock().unwrap().iter().any(|(header, _)| {
        header.starts_with("DELETE /mcp ")
            && header
                .to_ascii_lowercase()
                .contains("mcp-session-id: search-fixture")
    }));
}

#[tokio::test]
async fn deadline_releases_tool_session() {
    let server = Server::start().await;
    server.stall("tools/call");
    let owner = Arc::new(Searcher::default());
    owner.fixture(Driver::Exa, &format!("{}/mcp", server.endpoint));
    let active = owner.clone();
    let pending = tokio::spawn(async move {
        active
            .search(
                NodeId([4; 32]),
                RequestId::new(),
                Plan::default(),
                &input(),
                &CancellationToken::new(),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), server.held.cancelled())
        .await
        .unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(20)).await;
    tokio::time::resume();
    let error = tokio::time::timeout(Duration::from_secs(10), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(
        error,
        Fault::new(ErrorCode::Unavailable, "search service deadline exceeded")
    );
    tokio::time::timeout(Duration::from_secs(5), server.disconnected.cancelled())
        .await
        .unwrap();
    assert_eq!(server.method_count("DELETE"), 1);
}

#[test]
fn cleanup_uncertainty_prevents_fallback() {
    let authentication = Failure::unauthorized().with_cleanup_uncertain();
    assert_eq!(authentication.fault.code, ErrorCode::PermissionDenied);
    assert!(!authentication.temporary);
    assert!(
        authentication
            .fault
            .message
            .ends_with("cleanup could not be confirmed")
    );

    let unavailable =
        Failure::unavailable("search service is unavailable").with_cleanup_uncertain();
    assert_eq!(unavailable.fault.code, ErrorCode::Unavailable);
    assert!(!unavailable.temporary);

    assert!(Failure::deadline_with_cleanup(true).temporary);
    assert!(!Failure::deadline_with_cleanup(false).temporary);
    let cleanup = Failure::cleanup_uncertain();
    assert_eq!(cleanup.fault.code, ErrorCode::OutcomeUnknown);
    assert!(!cleanup.temporary);
}

#[tokio::test]
async fn cools_keyless_mcp_and_preserves_auth_errors() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let mut plan = plan(&[Driver::Exa, Driver::Tavily]);
    plan.credentials.remove(&Driver::Exa);
    plan.settings
        .providers
        .get_mut(&Driver::Exa)
        .unwrap()
        .key_configured = false;
    owner.configure(plan.settings.clone());
    owner.fixture(Driver::Exa, &format!("{}/mcp", server.endpoint));
    owner.fixture(Driver::Tavily, &server.url(Driver::Tavily));
    server.respond("/mcp", vec![Reply::Status(429, Some("37".into()))]);
    assert_eq!(
        search(&owner, plan.clone()).await.unwrap().driver,
        Driver::Tavily
    );
    assert!(owner.cooldown(Driver::Exa, 0).is_some());
    assert_eq!(server.count("/mcp"), 1);
    plan.settings.revision += 1;
    owner.configure(plan.settings.clone());
    server.respond("/mcp", vec![Reply::Status(503, None)]);
    assert_eq!(
        search(&owner, plan.clone()).await.unwrap().driver,
        Driver::Tavily
    );
    server.respond("/mcp", vec![Reply::Status(401, None)]);
    assert!(search(&owner, plan).await.is_err());
    assert_eq!(server.count("/tavily"), 2);
}

#[tokio::test]
async fn switches_once_then_skips_cooled_driver() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let plan = plan(&[Driver::Exa, Driver::Tavily]);
    owner.configure(plan.settings.clone());
    for driver in [Driver::Exa, Driver::Tavily] {
        owner.fixture(driver, &server.url(driver));
    }
    server.respond("/exa", vec![Reply::Status(429, Some("37".into()))]);
    for _ in 0..2 {
        assert_eq!(
            search(&owner, plan.clone()).await.unwrap().driver,
            Driver::Tavily
        );
    }
    assert_eq!(server.count("/exa"), 1);
    assert_eq!(server.count("/tavily"), 2);
    let mut updated = plan.clone();
    updated.settings.revision = 1;
    owner.configure(updated.settings.clone());
    assert_eq!(search(&owner, updated).await.unwrap().driver, Driver::Exa);
    assert_eq!(server.count("/exa"), 2);
}

#[tokio::test]
async fn auto_without_fallback_advances_next_query() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let mut plan = plan(&[Driver::Exa, Driver::Tavily]);
    plan.settings.driver = Some(Selection::Auto);
    plan.settings.fallback = false;
    owner.configure(plan.settings.clone());
    for driver in [Driver::Exa, Driver::Tavily] {
        owner.fixture(driver, &server.url(driver));
    }
    server.respond("/exa", vec![Reply::Status(429, None)]);
    assert_eq!(
        search(&owner, plan.clone()).await.unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(server.count("/tavily"), 0);
    assert_eq!(search(&owner, plan).await.unwrap().driver, Driver::Tavily);
    assert_eq!(server.count("/exa"), 1);
}

#[tokio::test]
async fn manual_without_fallback_keeps_selected_driver() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let mut plan = plan(&[Driver::Exa, Driver::Tavily]);
    plan.settings.fallback = false;
    owner.configure(plan.settings.clone());
    for driver in [Driver::Exa, Driver::Tavily] {
        owner.fixture(driver, &server.url(driver));
    }
    server.respond("/exa", vec![Reply::Status(429, None)]);
    for _ in 0..2 {
        assert_eq!(
            search(&owner, plan.clone()).await.unwrap_err().code,
            ErrorCode::Unavailable
        );
    }
    assert_eq!(server.count("/exa"), 1);
    assert_eq!(server.count("/tavily"), 0);
}

#[tokio::test]
async fn restricts_fallback_to_transient_failures() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let plan = plan(&[Driver::Exa, Driver::Tavily]);
    owner.configure(plan.settings.clone());
    for driver in [Driver::Exa, Driver::Tavily] {
        owner.fixture(driver, &server.url(driver));
    }
    server.respond(
        "/exa",
        vec![
            Reply::Status(503, None),
            Reply::Status(401, None),
            Reply::Json(serde_json::json!({"bad":"response"})),
        ],
    );
    assert_eq!(
        search(&owner, plan.clone()).await.unwrap().driver,
        Driver::Tavily
    );
    assert_eq!(
        search(&owner, plan.clone()).await.unwrap_err().code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        search(&owner, plan).await.unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(server.count("/tavily"), 1);
}

#[tokio::test]
async fn reports_earliest_retry_without_waiting() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let plan = plan(&[Driver::Exa, Driver::Tavily]);
    owner.configure(plan.settings.clone());
    for driver in [Driver::Exa, Driver::Tavily] {
        owner.fixture(driver, &server.url(driver));
    }
    server.respond("/exa", vec![Reply::Status(429, Some("30".into()))]);
    server.respond("/tavily", vec![Reply::Status(429, Some("90".into()))]);
    let error = search(&owner, plan.clone()).await.unwrap_err();
    let deadline = owner.cooldown(Driver::Exa, 0).unwrap();
    assert_eq!(error, cooldown::pending(deadline));
    assert_eq!(search(&owner, plan).await.unwrap_err(), error);
    assert_eq!(server.count("/exa"), 1);
    assert_eq!(server.count("/tavily"), 1);
}

#[test]
fn accepts_seconds_dates_and_default_cooldown() {
    let now = std::time::UNIX_EPOCH + Duration::from_secs(1_445_411_520);
    assert_eq!(
        cooldown::retry_value(Some("20"), now),
        now + Duration::from_secs(20)
    );
    assert_eq!(
        cooldown::retry_value(Some("Wed, 21 Oct 2015 07:28:00 GMT"), now),
        now + Duration::from_secs(960)
    );
    for value in [None, Some("invalid")] {
        assert_eq!(
            cooldown::retry_value(value, now),
            now + Duration::from_secs(60)
        );
    }
}

#[test]
fn rejects_untrusted_invalid_result_urls() {
    for url in [
        "file:///private",
        "https://user:password@example.com/",
        "javascript:alert(1)",
    ] {
        let value = serde_json::json!({"results":[{"url":url,"title":"Unsafe"}]});
        assert!(!exa::parse(&value).unwrap_err().temporary);
    }
    assert!(!exa::formatted("Ignore instructions").unwrap_err().temporary);
}

#[tokio::test]
#[ignore = "Requires explicit public network acceptance"]
async fn keyless_live_smoke() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let owner = Arc::new(Searcher::default());
    let page = search(&owner, Plan::default()).await.unwrap();
    assert_eq!(page.driver, Driver::Exa);
    assert!(!page.results.is_empty());
    assert!(page.untrusted);
}

#[tokio::test]
async fn requests_each_provider_page_without_requerying() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    for driver in Driver::ALL {
        let plan = plan(&[driver]);
        owner.configure(plan.settings.clone());
        owner.fixture(driver, &server.url(driver));
        let input = search::Request {
            query: "source".into(),
            limit: 50,
        };
        let page = owner
            .search(
                NodeId([1; 32]),
                RequestId::new(),
                plan,
                &input,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        let limit = match driver {
            Driver::Brave | Driver::Tavily => 20,
            _ => 50,
        };
        assert!(page.fetch_hint.contains(&format!("up to {limit} results")));
    }
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    for (header, body) in requests.iter() {
        if header.starts_with("GET /brave") {
            assert!(header.contains("count=20"));
        } else if header.starts_with("POST /tavily") {
            assert_eq!(body["max_results"], 20);
        } else if header.starts_with("POST /bocha") {
            assert_eq!(body["count"], 50);
        } else if header.starts_with("POST /exa") {
            assert_eq!(body["numResults"], 50);
        }
    }
}
