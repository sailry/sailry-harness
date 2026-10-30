use super::*;
use chrono::Utc;
use serde_json::{Value, json};

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let response = network::json(
        network::client()?
            .post(endpoint)
            .bearer_auth(network::key(auth)?)
            .json(&body(input)?),
    )
    .await?;
    parse(&response)
}

fn body(input: &search::Request) -> Result<Value, Failure> {
    if input.query.chars().count() > 128 {
        return Err(Failure::service_limit());
    }
    let mut query = filters::query(input);
    if query.chars().count() > 128 {
        query = input.query.clone();
    }
    if let Some(freshness) = input.freshness {
        let cutoff = filters::since(freshness, Utc::now());
        let filtered = format!("{query} after:{}", cutoff.format("%Y-%m-%d"));
        if filtered.chars().count() <= 128 {
            query = filtered;
        }
    }
    Ok(json!({"query":query,"count":input.limit.min(50),"need_content":false}))
}

pub(super) fn parse(value: &Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/results", "title", "snippet", "date")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_nullable_publication() {
        let results = parse(&json!({"results":[
            {"title":"Public source","url":"https://example.com/source","snippet":"Untrusted content","summary":"Relevant excerpt","content":"","date":"2026-10-30T11:30:00Z","authority":3,"site_name":null,"site_icon":null,"rerank_score":2.4},
            {"title":"Undated source","url":"https://docs.example.com/","snippet":"Undated excerpt","summary":"","content":"","date":null,"authority":-1,"site_name":null,"site_icon":null,"rerank_score":1.0}
        ]})).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].published.as_deref(),
            Some("2026-10-30T11:30:00Z")
        );
        assert!(results[1].published.is_none());
    }

    #[test]
    fn maps_query_operators() {
        let input = serde_json::from_value(json!({
            "query":"official release","freshness":"week","include_domains":["example.com"],
            "exclude_domains":["private.example.com"],"limit":50
        }))
        .unwrap();
        let body = body(&input).unwrap();
        let query = body["query"].as_str().unwrap();
        assert!(
            query.starts_with("official release site:example.com -site:private.example.com after:")
        );
        assert_eq!(body["count"], 50);
        assert_eq!(body["need_content"], false);
    }

    #[test]
    fn preserves_long_queries_without_native_filters() {
        let mut input: search::Request =
            serde_json::from_value(json!({"query":"a".repeat(128)})).unwrap();
        assert!(body(&input).is_ok());
        input.include_domains = vec!["example.com".into()];
        input.freshness = Some(search::Freshness::Week);
        assert_eq!(body(&input).unwrap()["query"], input.query);
        input.query.push('a');
        assert!(body(&input).is_err());
    }
}
