use super::*;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::Value;

pub(super) const MAX_BODY: usize = 1024 * 1024;

pub(super) fn client() -> Result<Client, Failure> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(18))
        .user_agent("Sailry/1")
        .build()
        .map_err(|_| Failure::unavailable("search HTTP client is unavailable"))
}

pub(super) async fn json(request: RequestBuilder) -> Result<Value, Failure> {
    json_with_limit(request, MAX_BODY).await
}

pub(super) async fn json_with_limit(
    request: RequestBuilder,
    limit: usize,
) -> Result<Value, Failure> {
    let mut response = request.send().await.map_err(|error| {
        if error.is_builder() {
            Failure::invalid("search request is invalid")
        } else {
            Failure::unavailable("search service is unreachable")
        }
    })?;
    let status = response.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        let deadline = cooldown::retry(response.headers(), SystemTime::now());
        return Err(Failure {
            fault: cooldown::pending(deadline),
            temporary: true,
            retry_at: Some(deadline),
            uncertain: false,
        });
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(Failure::unauthorized());
    }
    if status.is_server_error() {
        return Err(Failure::unavailable("search service is unavailable"));
    }
    if !status.is_success() {
        return Err(Failure::invalid("search service rejected the request"));
    }
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(Failure::invalid("search service response is too large"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Failure::unavailable("search response was interrupted"))?
    {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(Failure::invalid("search service response is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|_| Failure::invalid("search service returned invalid JSON"))
}

pub(super) fn key(auth: &Auth) -> Result<&str, Failure> {
    auth.key
        .as_ref()
        .map(Secret::expose)
        .ok_or_else(Failure::unauthorized)
}

pub(super) fn results(
    value: &Value,
    pointer: &str,
    title: &str,
    snippet: &str,
    date: &str,
) -> Result<Vec<search::Result>, Failure> {
    let entries = value
        .pointer(pointer)
        .and_then(Value::as_array)
        .ok_or_else(|| Failure::invalid("search service response has no result list"))?;
    entries
        .iter()
        .map(|value| {
            let url = value
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| Failure::invalid("search result URL is missing"))?;
            let parsed = url::Url::parse(url)
                .map_err(|_| Failure::invalid("search result URL is invalid"))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err(Failure::invalid("search result URL is invalid"));
            }
            Ok(search::Result {
                title: text(value.get(title).and_then(Value::as_str).unwrap_or(url), 512),
                url: url.into(),
                snippet: text(
                    value
                        .get(snippet)
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    4000,
                ),
                published: value
                    .get(date)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(|value| text(value, 128)),
            })
        })
        .collect()
}

pub(super) fn text(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
