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
    for driver in Driver::ALL {
        let plan = plan(&[driver]);
        owner.configure(plan.settings.clone());
        owner.fixture(driver, &server.url(driver));
        for (index, freshness) in ranges.iter().copied().enumerate() {
            let input = search::Request {
                freshness: Some(freshness),
                include_domains: vec!["example.com".into(), "docs.example.org".into()],
                exclude_domains: vec!["private.example.com".into()],
                ..input()
            };
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
                    assert_eq!(body["include"], "example.com|docs.example.org");
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
