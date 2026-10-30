use super::*;
use serde_json::{Value, json};

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    if input.query.chars().count() > 100 {
        return Err(Failure::service_limit());
    }
    let response = network::json(
        network::client()?
            .post(endpoint)
            .bearer_auth(network::key(auth)?)
            .json(&body(input)),
    )
    .await?;
    parse(&response)
}

fn body(input: &search::Request) -> Value {
    let mut body = json!({
        "Query":input.query, "SearchType":"web", "Count":input.limit.min(50),
        "Filter":{"NeedUrl":true}
    });
    if !input.include_domains.is_empty() {
        body["Filter"]["Sites"] = input.include_domains.join("|").into();
    }
    if !input.exclude_domains.is_empty() {
        body["Filter"]["BlockHosts"] = input
            .exclude_domains
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("|")
            .into();
    }
    if let Some(freshness) = input.freshness {
        body["TimeRange"] = match freshness {
            search::Freshness::Hour | search::Freshness::Day => "OneDay",
            search::Freshness::Week => "OneWeek",
            search::Freshness::Month => "OneMonth",
            search::Freshness::Year => "OneYear",
        }
        .into();
    }
    body
}

pub(super) fn parse(value: &Value) -> Result<Vec<search::Result>, Failure> {
    if let Some(error) = value
        .pointer("/ResponseMetadata/Error")
        .filter(|error| !error.is_null())
    {
        let code = error.get("CodeN").and_then(Value::as_u64).or_else(|| {
            error
                .get("Code")
                .and_then(Value::as_str)
                .and_then(|code| code.parse().ok())
        });
        return Err(match code {
            Some(10401 | 10403) => Failure::unauthorized(),
            Some(700429) => {
                let deadline = SystemTime::now() + Duration::from_secs(60);
                Failure {
                    fault: cooldown::pending(deadline),
                    temporary: true,
                    retry_at: Some(deadline),
                }
            }
            Some(10500) => Failure::unavailable("search service is unavailable"),
            _ => Failure::invalid("search service rejected the request"),
        });
    }
    let Some(entries) = value
        .pointer("/Result/WebResults")
        .and_then(Value::as_array)
    else {
        if value.pointer("/Result/ResultCount").and_then(Value::as_u64) == Some(0) {
            return Ok(Vec::new());
        }
        return Err(Failure::invalid(
            "search service response has no result list",
        ));
    };
    let normalized = json!({"results":entries.iter().map(|entry| {
        let snippet = entry.get("Summary").and_then(Value::as_str).filter(|summary| !summary.is_empty())
            .or_else(|| entry.get("Snippet").and_then(Value::as_str)).unwrap_or_default();
        json!({
            "url":entry.get("Url"),"title":entry.get("Title"),
            "snippet":snippet,"published":entry.get("PublishTime")
        })
    }).collect::<Vec<_>>()});
    network::results(&normalized, "/results", "title", "snippet", "published")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_relevant_source_excerpt() {
        let results = parse(&json!({"ResponseMetadata":{"RequestId":"fixture"},"Result":{"ResultCount":1,"WebResults":[{
            "Url":"https://example.com/source","Title":"Public source","Snippet":"Short excerpt",
            "Summary":"Relevant source excerpt","PublishTime":"2026-10-30T19:30:00+08:00"
        }]}})).unwrap();
        assert_eq!(results[0].title, "Public source");
        assert_eq!(results[0].snippet, "Relevant source excerpt");
        assert_eq!(
            results[0].published.as_deref(),
            Some("2026-10-30T19:30:00+08:00")
        );
    }

    #[test]
    fn separates_provider_errors() {
        for code in [10401, 10403, 10406, 10412, 10500, 700429] {
            let error = parse(&json!({"ResponseMetadata":{"Error":{"CodeN":code,"Code":code.to_string(),"Message":"fixture-key"}},"Result":null})).unwrap_err();
            assert!(!error.fault.message.contains("fixture-key"));
            assert_eq!(error.retry_at.is_some(), code == 700429);
            assert_eq!(error.temporary, matches!(code, 10500 | 700429));
            if matches!(code, 10401 | 10403) {
                assert_eq!(error.fault.code, ErrorCode::PermissionDenied);
            }
        }
    }

    #[test]
    fn accepts_empty_web_results() {
        assert!(
            parse(&json!({"ResponseMetadata":{},"Result":{"ResultCount":0,"WebResults":null}}))
                .unwrap()
                .is_empty()
        );
        assert!(parse(&json!({"ResponseMetadata":{},"Result":null})).is_err());
    }

    #[test]
    fn bounds_native_exclusions() {
        let input = serde_json::from_value(json!({
            "query":"official release","freshness":"hour","include_domains":["example.com"],
            "exclude_domains":["one.example","two.example","three.example","four.example","five.example","six.example"]
        })).unwrap();
        let body = body(&input);
        assert_eq!(body["TimeRange"], "OneDay");
        assert_eq!(body["Filter"]["NeedUrl"], true);
        assert_eq!(body["Filter"]["Sites"], "example.com");
        assert_eq!(
            body["Filter"]["BlockHosts"],
            "one.example|two.example|three.example|four.example|five.example"
        );
        assert_eq!(input.exclude_domains.len(), 6);
    }
}
