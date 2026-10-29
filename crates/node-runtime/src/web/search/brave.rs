use super::*;

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let mut parameters = vec![
        ("q", filters::query(input)),
        ("count", input.limit.to_string()),
    ];
    if let Some(freshness) = input.freshness {
        parameters.push((
            "freshness",
            match freshness {
                search::Freshness::Hour | search::Freshness::Day => "pd",
                search::Freshness::Week => "pw",
                search::Freshness::Month => "pm",
                search::Freshness::Year => "py",
            }
            .into(),
        ));
    }
    let response = network::json(
        network::client()?
            .get(endpoint)
            .header("X-Subscription-Token", network::key(auth)?)
            .query(&parameters),
    )
    .await?;
    parse(&response)
}

pub(super) fn parse(value: &serde_json::Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/web/results", "title", "description", "page_age")
}
