use super::*;
use chrono::Utc;
use serde_json::Value;

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    let response = network::json(
        network::client()?
            .get(endpoint)
            .header("X-API-Key", network::key(auth)?)
            .query(&parameters(input)),
    )
    .await?;
    parse(&response)
}

fn parameters(input: &search::Request) -> Vec<(&'static str, String)> {
    let mut parameters = vec![("query", input.query.clone())];
    if !input.include_domains.is_empty() {
        parameters.push(("include_domains", input.include_domains.join(",")));
    }
    if !input.exclude_domains.is_empty() {
        parameters.push(("exclude_domains", input.exclude_domains.join(",")));
    }
    if let Some(freshness) = input.freshness {
        let now = Utc::now();
        let minutes = (now - filters::since(freshness, now)).num_minutes();
        parameters.push(("recency_minutes", minutes.to_string()));
    }
    parameters
}

pub(super) fn parse(value: &Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/results", "title", "snippet", "date")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn retains_missing_publication() {
        let results = parse(&json!({"query":"example","page":0,"results":[
            {"title":"Public source","url":"https://example.com/source","snippet":"Untrusted content","date":"2026-10-30T11:30:00Z"},
            {"title":"Undated source","url":"https://docs.example.com/","snippet":"Source excerpt"}
        ]})).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].published.as_deref(),
            Some("2026-10-30T11:30:00Z")
        );
        assert_eq!(results[1].published, None);
    }

    #[test]
    fn omits_unsupported_result_count() {
        let input = serde_json::from_value(json!({
            "query":"official release", "limit":20, "freshness":"hour",
            "include_domains":["example.com","docs.example.org"],
            "exclude_domains":["private.example.com"]
        }))
        .unwrap();
        let parameters = parameters(&input);
        assert!(parameters.contains(&("recency_minutes", "60".into())));
        assert!(parameters.contains(&("include_domains", "example.com,docs.example.org".into())));
        assert!(parameters.contains(&("exclude_domains", "private.example.com".into())));
        assert!(
            !parameters
                .iter()
                .any(|(name, _)| matches!(*name, "count" | "num_results" | "limit"))
        );
    }
}
