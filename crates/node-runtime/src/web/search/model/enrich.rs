//! Citation excerpts come from bounded public page reads, never generated answer spans.
use super::*;
use futures::{StreamExt, stream};
use sailry_protocol::web;
use std::collections::BTreeSet;

pub(super) fn select(results: &mut Vec<search::Result>, input: &search::Request) {
    results.retain(|result| {
        let redirect = url::Url::parse(&result.url).ok().is_some_and(|url| {
            url.host_str() == Some("vertexaisearch.cloud.google.com")
                && url.path().starts_with("/grounding-api-redirect/")
        });
        let mut input = input.clone();
        if redirect {
            // Grounding redirect hosts do not identify the cited source domain.
            input.include_domains.clear();
            input.exclude_domains.clear();
        }
        let mut candidate = vec![result.clone()];
        super::filters::apply(&mut candidate, &input, Utc::now());
        !candidate.is_empty()
    });
    results.truncate(input.limit as usize);
}

pub(super) async fn sources(
    results: &mut Vec<search::Result>,
    context: &Context<'_>,
    stop: &CancellationToken,
) -> Result<(), Failure> {
    let cancelled = stop.child_token();
    let jobs: Vec<_> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.snippet.is_empty())
        .map(|(index, result)| (index, result.url.clone()))
        .collect();
    let fetch = context.ingress.web.clone();
    let caller = context.caller;
    let pending = cancelled.clone();
    let reads = stream::iter(jobs)
        .map(move |(index, url)| {
            let fetch = fetch.clone();
            let stop = pending.clone();
            async move {
                let input = web::Request { url, offset: 0 };
                (
                    index,
                    fetch.fetch(caller, RequestId::new(), &input, &stop).await,
                )
            }
        })
        .buffer_unordered(4);
    let operation = async {
        futures::pin_mut!(reads);
        while let Some((index, result)) = reads.next().await {
            if let Ok(page) = result {
                apply(&mut results[index], page);
            }
        }
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => {
            cancelled.cancel();
            return Err(Failure::cancelled());
        }
        _ = tokio::time::timeout(Duration::from_secs(10), operation) => {}
    }
    cancelled.cancel();
    let mut urls = BTreeSet::new();
    results.retain(|result| urls.insert(result.url.clone()));
    Ok(())
}

fn apply(result: &mut search::Result, page: web::Page) {
    if !(200..300).contains(&page.status) {
        return;
    }
    let Ok(url) = reqwest::Url::parse(&page.url) else {
        return;
    };
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return;
    }
    result.url = url.to_string();
    result.snippet = super::super::network::text(&page.content, 4000);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result() -> search::Result {
        search::Result {
            title: "Citation title".into(),
            url: "https://redirect.example/source".into(),
            snippet: String::new(),
            published: None,
        }
    }

    fn page() -> web::Page {
        web::Page {
            url: "https://official.example/final".into(),
            status: 200,
            content_type: "text/html".into(),
            content: "Actual source excerpt".into(),
            title: Some("Fetched page title".into()),
            next_offset: None,
            truncated: false,
            untrusted: true,
        }
    }

    #[test]
    fn filters_known_hosts_before_reading_sources() {
        let input = search::Request {
            include_domains: vec!["allowed.example".into()],
            exclude_domains: vec!["private.allowed.example".into()],
            limit: 2,
            ..Default::default()
        };
        let mut results = [
            "https://other.example/source",
            "https://private.allowed.example/source",
            "https://allowed.example/first",
            "https://vertexaisearch.cloud.google.com/grounding-api-redirect/opaque",
            "https://allowed.example/last",
        ]
        .into_iter()
        .map(|url| search::Result {
            url: url.into(),
            ..result()
        })
        .collect();
        select(&mut results, &input);
        assert_eq!(
            results
                .iter()
                .map(|result| result.url.as_str())
                .collect::<Vec<_>>(),
            [
                "https://allowed.example/first",
                "https://vertexaisearch.cloud.google.com/grounding-api-redirect/opaque",
            ]
        );
    }

    #[test]
    fn uses_actual_content_and_final_url() {
        let mut result = result();
        apply(&mut result, page());
        assert_eq!(result.title, "Citation title");
        assert_eq!(result.url, "https://official.example/final");
        assert_eq!(result.snippet, "Actual source excerpt");
    }

    #[test]
    fn preserves_empty_excerpt_on_failed_reads() {
        for (status, url) in [
            (404, "https://official.example/final"),
            (200, "file:///private/source"),
            (200, "https://name:private@official.example/final"),
        ] {
            let mut result = result();
            let mut page = page();
            page.status = status;
            page.url = url.into();
            apply(&mut result, page);
            assert_eq!(result.url, "https://redirect.example/source");
            assert!(result.snippet.is_empty());
        }
    }

    #[test]
    fn bounds_excerpts_at_unicode_characters() {
        let mut result = result();
        let mut page = page();
        page.content = "界".repeat(4001);
        apply(&mut result, page);
        assert_eq!(result.snippet.chars().count(), 4000);
    }
}
