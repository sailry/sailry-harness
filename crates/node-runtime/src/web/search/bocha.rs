use super::*;
use serde_json::{Value, json};

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let response = network::json(network::client()?.post(endpoint).bearer_auth(network::key(auth)?).json(&json!({
        "query": input.query, "count": input.limit, "freshness": "noLimit", "summary": false,
    }))).await?;
    parse(&response)
}

pub(super) fn parse(value: &Value) -> Result<Vec<search::Result>, Failure> {
    if let Some(code) = value.get("code").and_then(Value::as_u64)
        && code != 200
    {
        return Err(if code == 429 {
            let deadline = SystemTime::now() + Duration::from_secs(60);
            Failure {
                fault: cooldown::pending(deadline),
                temporary: true,
                retry_at: Some(deadline),
            }
        } else if code >= 500 {
            Failure::unavailable("search service is unavailable")
        } else if code == 401 || code == 403 {
            Failure::unauthorized()
        } else {
            Failure::invalid("search service rejected the request")
        });
    }
    network::results(
        value,
        "/data/webPages/value",
        "name",
        "snippet",
        "datePublished",
    )
}
