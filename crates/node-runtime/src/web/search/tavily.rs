use super::*;
use serde_json::json;

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let response = network::json(
        network::client()?
            .post(endpoint)
            .bearer_auth(network::key(auth)?)
            .json(&json!({
                "query": input.query, "max_results": input.limit, "search_depth": "basic",
                "include_answer": false, "include_raw_content": false,
            })),
    )
    .await?;
    parse(&response)
}

pub(super) fn parse(value: &serde_json::Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/results", "title", "content", "published_date")
}
