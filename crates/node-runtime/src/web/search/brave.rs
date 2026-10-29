use super::*;

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let mut parameters = vec![("q", query(input)?), ("count", input.limit.to_string())];
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

fn query(input: &search::Request) -> Result<String, Failure> {
    let fits = |query: &str| query.chars().count() <= 600 && query.split_whitespace().count() <= 75;
    if !fits(&input.query) {
        return Err(Failure::invalid("search query exceeds the service limit"));
    }
    let filtered = filters::query(input);
    Ok(if fits(&filtered) {
        filtered
    } else {
        input.query.clone()
    })
}

pub(super) fn parse(value: &serde_json::Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/web/results", "title", "description", "page_age")
}
