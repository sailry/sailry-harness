use super::*;

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let response = network::json(
        network::client()?
            .get(endpoint)
            .header("X-Subscription-Token", network::key(auth)?)
            .query(&[
                ("q", input.query.clone()),
                ("count", input.limit.to_string()),
            ]),
    )
    .await?;
    parse(&response)
}

pub(super) fn parse(value: &serde_json::Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/web/results", "title", "description", "page_age")
}
