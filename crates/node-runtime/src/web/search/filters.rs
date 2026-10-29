//! Native filters are widened where necessary, then checked against public result metadata.
use chrono::{DateTime, Duration, NaiveDate, Utc};
use sailry_protocol::search::{Freshness, Request, Result};

pub(super) fn since(freshness: Freshness, now: DateTime<Utc>) -> DateTime<Utc> {
    now - match freshness {
        Freshness::Hour => Duration::hours(1),
        Freshness::Day => Duration::days(1),
        Freshness::Week => Duration::days(7),
        Freshness::Month => Duration::days(31),
        Freshness::Year => Duration::days(365),
    }
}

pub(super) fn query(input: &Request) -> String {
    let mut query = input.query.clone();
    if !input.include_domains.is_empty() {
        let sites = input
            .include_domains
            .iter()
            .map(|domain| format!("site:{domain}"))
            .collect::<Vec<_>>();
        query.push(' ');
        if sites.len() == 1 {
            query.push_str(&sites[0]);
        } else {
            query.push('(');
            query.push_str(&sites.join(" OR "));
            query.push(')');
        }
    }
    for domain in &input.exclude_domains {
        query.push_str(" -site:");
        query.push_str(domain);
    }
    query
}

pub(super) fn apply(results: &mut Vec<Result>, input: &Request, now: DateTime<Utc>) {
    let cutoff = input.freshness.map(|freshness| since(freshness, now));
    results.retain(|result| {
        let Some(host) = url::Url::parse(&result.url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
        else {
            return false;
        };
        let matches = |domain: &String| host == *domain || host.ends_with(&format!(".{domain}"));
        if input.exclude_domains.iter().any(matches)
            || (!input.include_domains.is_empty() && !input.include_domains.iter().any(matches))
        {
            return false;
        }
        let Some((published, cutoff)) = result.published.as_deref().zip(cutoff) else {
            return true;
        };
        if let Ok(date) = NaiveDate::parse_from_str(published, "%Y-%m-%d") {
            // Date-only metadata cannot resolve a narrower time within the same day.
            return date >= cutoff.date_naive();
        }
        DateTime::parse_from_rfc3339(published)
            .or_else(|_| DateTime::parse_from_rfc2822(published))
            .map_or(true, |date| date >= cutoff)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Request {
        serde_json::from_value(serde_json::json!({
            "query":"official release", "freshness":"hour",
            "include_domains":["example.com"],"exclude_domains":["private.example.com"]
        }))
        .unwrap()
    }

    fn result(url: &str, published: Option<&str>) -> Result {
        Result {
            title: "Source".into(),
            url: url.into(),
            snippet: "Excerpt".into(),
            published: published.map(str::to_owned),
        }
    }

    #[test]
    fn narrows_domains_and_known_dates() {
        let now = DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
            .unwrap()
            .to_utc();
        let mut results = vec![
            result("https://example.com/current", Some("2026-10-30T11:10:00Z")),
            result("https://docs.example.com/undated", None),
            result("https://example.com/date", Some("2026-10-30")),
            result("https://example.com/unparsed", Some("unknown")),
            result("https://example.com/old", Some("2026-10-30T10:59:59Z")),
            result("https://example.com/previous", Some("2026-10-29")),
            result("https://private.example.com/", None),
            result("https://sub.private.example.com/", None),
            result("https://anotherexample.com/", None),
            result("https://example.com.evil.test/", None),
        ];
        apply(&mut results, &input(), now);
        assert_eq!(results.len(), 4);
        assert_eq!(results[1].url, "https://docs.example.com/undated");
    }

    #[test]
    fn joins_allowed_sites_with_or() {
        let mut input = input();
        input.include_domains.push("other.example".into());
        assert_eq!(
            query(&input),
            "official release (site:example.com OR site:other.example) -site:private.example.com"
        );
        input.include_domains.clear();
        assert_eq!(query(&input), "official release -site:private.example.com");
    }

    #[test]
    fn maps_all_ranges() {
        let now = DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
            .unwrap()
            .to_utc();
        for (freshness, hours) in [
            (Freshness::Hour, 1),
            (Freshness::Day, 24),
            (Freshness::Week, 168),
            (Freshness::Month, 744),
            (Freshness::Year, 8760),
        ] {
            assert_eq!((now - since(freshness, now)).num_hours(), hours);
        }
    }
}
