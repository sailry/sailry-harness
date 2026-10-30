use super::*;
use sailry_protocol::search::Freshness;
use serde_json::json;

#[tokio::test]
async fn maps_time_and_domain_filters() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let ranges = [
        Freshness::Hour,
        Freshness::Day,
        Freshness::Week,
        Freshness::Month,
        Freshness::Year,
    ];
    for driver in Driver::ALL
        .into_iter()
        .filter(|driver| *driver != Driver::Model)
    {
        let plan = plan(&[driver]);
        owner.configure(plan.settings.clone());
        owner.fixture(driver, &server.url(driver));
        for (index, freshness) in ranges.iter().copied().enumerate() {
            let input = search::Request {
                freshness: Some(freshness),
                include_domains: vec!["example.com".into(), "docs.example.org".into()],
                exclude_domains: vec!["private.example.com".into()],
                ..input()
            }
            .normalized();
            owner
                .search(
                    NodeId([1; 32]),
                    RequestId::new(),
                    plan.clone(),
                    &input,
                    &CancellationToken::new(),
                )
                .await
                .unwrap();
            let (header, body) = server.requests.lock().unwrap().last().unwrap().clone();
            let first = header.lines().next().unwrap();
            let path = first.split_whitespace().nth(1).unwrap();
            let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
            let parameters = url.query_pairs().collect::<BTreeMap<_, _>>();
            match driver {
                Driver::Model => {
                    unreachable!("native model requests have separate transport fixtures")
                }
                Driver::Volcengine => {
                    assert_eq!(
                        body["TimeRange"],
                        ["OneDay", "OneDay", "OneWeek", "OneMonth", "OneYear"][index]
                    );
                    assert_eq!(body["Filter"]["Sites"], input.include_domains.join("|"));
                    assert_eq!(
                        body["Filter"]["BlockHosts"],
                        input.exclude_domains.join("|")
                    );
                }
                Driver::Deepseek => {
                    let query = body["query"].as_str().unwrap();
                    assert!(query.starts_with(&filters::query(&input)));
                    assert!(query.contains(" after:"));
                }
                Driver::Tinyfish => {
                    assert_eq!(
                        parameters["include_domains"],
                        input.include_domains.join(",")
                    );
                    assert_eq!(
                        parameters["exclude_domains"],
                        input.exclude_domains.join(",")
                    );
                    assert_eq!(
                        parameters["recency_minutes"],
                        [60, 1440, 10080, 44640, 525600][index].to_string()
                    );
                }
                Driver::Exa => {
                    assert_eq!(body["includeDomains"], json!(input.include_domains));
                    assert_eq!(body["excludeDomains"], json!(input.exclude_domains));
                    let start = chrono::DateTime::parse_from_rfc3339(
                        body["startPublishedDate"].as_str().unwrap(),
                    )
                    .unwrap()
                    .to_utc();
                    let expected = filters::since(freshness, chrono::Utc::now());
                    assert!((expected - start).num_seconds().abs() < 5);
                }
                Driver::Tavily => {
                    assert_eq!(
                        body["time_range"],
                        ["day", "day", "week", "month", "year"][index]
                    );
                    assert_eq!(body["include_domains"], json!(input.include_domains));
                    assert_eq!(body["exclude_domains"], json!(input.exclude_domains));
                    assert_eq!(body["include_published_date"], true);
                }
                Driver::Bocha => {
                    assert_eq!(
                        body["freshness"],
                        ["oneDay", "oneDay", "oneWeek", "oneMonth", "oneYear"][index]
                    );
                    assert_eq!(body["include"], input.include_domains.join("|"));
                    assert_eq!(body["exclude"], "private.example.com");
                }
                Driver::Brave | Driver::Searxng => {
                    assert_eq!(parameters["q"], filters::query(&input));
                    if driver == Driver::Brave {
                        assert_eq!(
                            parameters["freshness"],
                            ["pd", "pd", "pw", "pm", "py"][index]
                        );
                    } else {
                        assert_eq!(
                            parameters["time_range"],
                            ["day", "day", "month", "month", "year"][index]
                        );
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn scopes_keyless_exa_queries() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    owner.fixture(Driver::Exa, &format!("{}/mcp", server.endpoint));
    let input = search::Request {
        include_domains: vec!["example.com".into()],
        exclude_domains: vec!["private.example.com".into()],
        freshness: Some(Freshness::Year),
        ..input()
    };
    owner
        .search(
            NodeId([1; 32]),
            RequestId::new(),
            Plan::default(),
            &input,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let requests = server.requests.lock().unwrap();
    let call = requests
        .iter()
        .find(|(_, body)| body["method"] == "tools/call")
        .unwrap();
    assert_eq!(
        call.1["params"]["arguments"]["query"],
        filters::query(&input)
    );
    assert!(
        call.1["params"]["arguments"]
            .get("includeDomains")
            .is_none()
    );
}

#[tokio::test]
async fn bounds_brave_queries_without_losing_filters() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let plan = plan(&[Driver::Brave]);
    owner.configure(plan.settings.clone());
    owner.fixture(Driver::Brave, &server.url(Driver::Brave));
    let mut included = vec!["example.com".into()];
    included.extend((0..19).map(|index| {
        format!(
            "host-{index}.{}.{}.example.org",
            "a".repeat(60),
            "b".repeat(60)
        )
    }));
    let cases = [
        search::Request {
            query: "source".into(),
            include_domains: included,
            exclude_domains: vec!["private.example.com".into()],
            ..Default::default()
        },
        search::Request {
            query: std::iter::repeat_n("a", 74).collect::<Vec<_>>().join(" "),
            include_domains: vec!["example.com".into()],
            exclude_domains: vec!["private.example.com".into()],
            ..Default::default()
        },
    ];
    assert!(filters::query(&cases[0]).chars().count() > 600);
    assert!(filters::query(&cases[1]).chars().count() < 600);
    assert!(filters::query(&cases[1]).split_whitespace().count() > 75);
    for (index, input) in cases.into_iter().enumerate() {
        input.validate().unwrap();
        server.respond(
            "/brave",
            vec![Reply::Json(json!({"web":{"results":[
                {"title":"Allowed","url":"https://example.com/source","description":"Excerpt"},
                {"title":"Excluded","url":"https://private.example.com/source","description":"Excerpt"},
                {"title":"Outside","url":"https://unrelated.example/source","description":"Excerpt"}
            ]}}))],
        );
        let page = owner
            .search(
                NodeId([1; 32]),
                RequestId::new(),
                plan.clone(),
                &input,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(page.results.len(), 1);
        assert_eq!(page.results[0].url, "https://example.com/source");
        assert_eq!(server.count("/brave"), index + 1);
        let (header, _) = server.requests.lock().unwrap().last().unwrap().clone();
        let path = header.split_whitespace().nth(1).unwrap();
        let url = url::Url::parse(&format!("http://fixture{path}")).unwrap();
        let parameters = url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(parameters["q"], input.query);
    }
}

#[tokio::test]
async fn rejects_brave_queries_before_sending() {
    let server = Server::start().await;
    let owner = Arc::new(Searcher::default());
    let plan = plan(&[Driver::Brave]);
    owner.configure(plan.settings.clone());
    owner.fixture(Driver::Brave, &server.url(Driver::Brave));
    for query in [
        "a".repeat(601),
        std::iter::repeat_n("a", 76).collect::<Vec<_>>().join(" "),
    ] {
        let input = search::Request {
            query,
            ..Default::default()
        };
        input.validate().unwrap();
        let error = owner
            .search(
                NodeId([1; 32]),
                RequestId::new(),
                plan.clone(),
                &input,
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert_eq!(error.message, "search query exceeds the service limit");
    }
    assert_eq!(server.count("/brave"), 0);
}
