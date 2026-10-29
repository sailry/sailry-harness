use super::*;
use serde_json::json;

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let mut body = json!({
        "query": input.query, "max_results": input.limit, "search_depth": "basic",
        "include_answer": false, "include_raw_content": false,
        "include_published_date": true,
    });
    if let Some(freshness) = input.freshness {
        body["time_range"] = json!(match freshness {
            search::Freshness::Hour | search::Freshness::Day => "day",
            search::Freshness::Week => "week",
            search::Freshness::Month => "month",
            search::Freshness::Year => "year",
        });
    }
    if !input.include_domains.is_empty() {
        body["include_domains"] = json!(input.include_domains);
    }
    if !input.exclude_domains.is_empty() {
        body["exclude_domains"] = json!(input.exclude_domains);
    }
    let response = network::json(
        network::client()?
            .post(endpoint)
            .bearer_auth(network::key(auth)?)
            .json(&body),
    )
    .await?;
    parse(&response)
}

pub(super) fn parse(value: &serde_json::Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/results", "title", "content", "published_date")
}
