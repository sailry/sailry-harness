use super::{Fetcher, cache::Document, network, policy};
use crate::web::search;
use sailry_protocol::{ErrorCode, Fault, Secret, web};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const MIN_CONTENT_CHARS: usize = 200;

pub(crate) struct Renderer {
    pub(in crate::web) endpoint: String,
    pub(in crate::web) key: Secret,
}

pub(super) fn needed(document: &Document) -> bool {
    (200..300).contains(&document.status)
        && matches!(
            document.content_type.as_str(),
            "text/html" | "application/xhtml+xml"
        )
        && document
            .content
            .trim()
            .chars()
            .take(MIN_CONTENT_CHARS)
            .count()
            < MIN_CONTENT_CHARS
}

impl Renderer {
    pub(super) async fn fetch(
        &self,
        owner: &Fetcher,
        direct: &Document,
        remaining: Duration,
    ) -> Result<Document, Fault> {
        if remaining.is_zero() {
            return Err(unavailable("web fetch deadline exceeded"));
        }
        let request = search::http_client()?
            .post(&self.endpoint)
            .header("X-API-Key", self.key.expose())
            .timeout(remaining)
            .json(&json!({
                "urls":[direct.url], "format":"markdown", "ttl":0,
                "per_url_timeout_ms":remaining.as_millis().clamp(1,110_000) as u64,
            }));
        let response = search::json_request(request, web::MAX_BODY_BYTES)
            .await
            .map_err(|error| {
                if error.code == ErrorCode::PermissionDenied {
                    unavailable("web rendering authentication failed")
                } else {
                    error
                }
            })?;
        let rendered = parse(&response, direct)?;
        network::validate(owner, &policy::url(&rendered.url)?).await?;
        Ok(rendered)
    }
}

fn parse(value: &Value, direct: &Document) -> Result<Document, Fault> {
    let results = value
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| unavailable("rendered web response is invalid"))?;
    let errors = value
        .get("errors")
        .and_then(Value::as_array)
        .ok_or_else(|| unavailable("rendered web response is invalid"))?;
    if let Some(error) = errors
        .iter()
        .find(|error| error.get("url").and_then(Value::as_str) == Some(direct.url.as_str()))
    {
        return Err(target_error(error));
    }
    if results.is_empty() {
        return Err(errors
            .first()
            .map(target_error)
            .unwrap_or_else(|| unavailable("rendered web page has no content")));
    }
    if results.len() != 1 {
        return Err(unavailable("rendered web response is invalid"));
    }
    let result = &results[0];
    if result.get("url").and_then(Value::as_str) != Some(direct.url.as_str()) {
        return Err(unavailable(
            "rendered web response does not match the requested page",
        ));
    }
    let final_url = result
        .get("final_url")
        .and_then(Value::as_str)
        .ok_or_else(|| unavailable("rendered web response has no final URL"))?;
    let final_url = policy::url(final_url)?;
    let content = result
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| unavailable("rendered web response has no page content"))?;
    if content.len() > web::MAX_BODY_BYTES {
        return Err(unavailable("web content exceeds the size limit"));
    }
    if content.trim().is_empty() {
        return Err(unavailable("rendered web page has no content"));
    }
    Ok(Document {
        url: final_url.into(),
        status: direct.status,
        content_type: "text/markdown".into(),
        title: result
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| direct.title.clone()),
        content: content.into(),
        fetched: Instant::now(),
    })
}

fn target_error(value: &Value) -> Fault {
    match value.get("error").and_then(Value::as_str) {
        Some("invalid_url" | "invalid_redirect_url") => policy::denied(),
        Some("content_too_large") => unavailable("web content exceeds the size limit"),
        Some("timeout") => unavailable("rendered web page timed out"),
        Some("login_required" | "bot_blocked") => {
            unavailable("rendered web page is not publicly available")
        }
        Some("page_not_found") => {
            Fault::new(ErrorCode::NotFound, "rendered web page was not found")
        }
        Some("empty_content") => unavailable("rendered web page has no content"),
        _ => unavailable("web rendering failed"),
    }
}

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}

#[cfg(test)]
mod tests;
