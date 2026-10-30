//! Native search accepts JSON or a bounded Responses event stream without exposing diagnostics.
use super::*;
use eventsource_stream::Eventsource;
use futures::{Stream, StreamExt};
use reqwest::{Response, StatusCode, header::CONTENT_TYPE};
use std::time::SystemTime;

pub(super) async fn read(response: Response, api: ModelApi) -> Result<Value, Failure> {
    let status = response.status();
    let retry = super::super::cooldown::retry(response.headers(), SystemTime::now());
    let streaming = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
        });
    if response
        .content_length()
        .is_some_and(|size| size > super::super::network::MAX_BODY as u64)
    {
        return Err(invalid("model search response is too large"));
    }
    if status.is_success() && streaming && api == ModelApi::Responses {
        return events(response.bytes_stream()).await;
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| interrupted())? {
        if chunk.len() > super::super::network::MAX_BODY.saturating_sub(bytes.len()) {
            return Err(invalid("model search response is too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice::<Value>(&bytes);
    if !status.is_success() {
        return Err(rejected(status, value.as_ref().ok(), retry));
    }
    value.map_err(|_| invalid("model search service returned invalid JSON"))
}

async fn events<S, B, E>(stream: S) -> Result<Value, Failure>
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
{
    let mut size = 0usize;
    let chunks = stream.map(move |chunk| {
        let chunk =
            chunk.map_err(|_| std::io::Error::other("model search stream was interrupted"))?;
        size = size.saturating_add(chunk.as_ref().len());
        if size > super::super::network::MAX_BODY {
            return Err(std::io::Error::other(
                "model search stream exceeded its limit",
            ));
        }
        Ok(chunk)
    });
    let stream = chunks.eventsource();
    futures::pin_mut!(stream);
    while let Some(event) = stream.next().await {
        let event = event.map_err(|_| interrupted())?;
        if event.data == "[DONE]" {
            break;
        }
        let value: Value = serde_json::from_str(&event.data)
            .map_err(|_| invalid("model search stream contains invalid JSON"))?;
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(&event.event)
        {
            "response.completed" => {
                return value
                    .get("response")
                    .cloned()
                    .ok_or_else(|| invalid("model search stream has no completed response"));
            }
            "response.failed" => {
                return Err(upstream(
                    value.pointer("/response/error").unwrap_or(&Value::Null),
                ));
            }
            "error" => return Err(upstream(value.get("error").unwrap_or(&value))),
            "response.incomplete" => return Err(invalid("model search response did not complete")),
            _ => {}
        }
    }
    Err(interrupted())
}

fn rejected(status: StatusCode, value: Option<&Value>, retry: SystemTime) -> Failure {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return limited(retry);
    }
    if matches!(status.as_u16(), 400 | 403 | 404 | 422 | 501)
        && value.is_some_and(|value| unsupported_error(value.get("error").unwrap_or(value)))
    {
        return unsupported();
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return request_fault(
            ErrorCode::PermissionDenied,
            ModelFailure::Unauthorized,
            "model search authentication failed",
            false,
        );
    }
    if status.is_server_error() {
        return request_fault(
            ErrorCode::Unavailable,
            ModelFailure::Request,
            "model search service is unavailable",
            true,
        );
    }
    request_fault(
        ErrorCode::InvalidRequest,
        ModelFailure::Request,
        "model search service rejected the request",
        false,
    )
}

pub(super) fn upstream(error: &Value) -> Failure {
    if unsupported_error(error) {
        return unsupported();
    }
    let code = error
        .get("error_code")
        .or_else(|| error.get("code"))
        .or_else(|| error.get("type"))
        .or_else(|| error.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match code {
        "too_many_requests" | "rate_limit_error" | "rate_limit_exceeded" | "RESOURCE_EXHAUSTED" => {
            limited(SystemTime::now() + Duration::from_secs(60))
        }
        "authentication_error" | "invalid_api_key" | "UNAUTHENTICATED" => request_fault(
            ErrorCode::PermissionDenied,
            ModelFailure::Unauthorized,
            "model search authentication failed",
            false,
        ),
        "unavailable" | "api_error" | "server_error" | "overloaded_error" | "UNAVAILABLE" => {
            request_fault(
                ErrorCode::Unavailable,
                ModelFailure::Request,
                "model search service is unavailable",
                true,
            )
        }
        _ => request_fault(
            ErrorCode::InvalidRequest,
            ModelFailure::Request,
            "native web search failed",
            false,
        ),
    }
}

fn unsupported_error(error: &Value) -> bool {
    let code = error
        .get("code")
        .or_else(|| error.get("type"))
        .or_else(|| error.get("status"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(
        code,
        "unsupported_tool" | "tool_not_supported" | "web_search_not_supported"
    ) {
        return true;
    }
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let parameter = error
        .get("param")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if code == "unsupported_parameter" && parameter == "tools" {
        return true;
    }
    let message = message
        .chars()
        .map(|character| match character {
            '\'' | '"' | '`' | ':' => ' ',
            _ => character,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    [
        "web_search",
        "web_search_20250305",
        "web search",
        "google_search",
        "googlesearch",
        "google search",
        "search grounding",
        "grounding with google search",
    ]
    .iter()
    .any(|name| {
        let passive = ["", " tool"].iter().any(|suffix| {
            [
                "not supported",
                "not available",
                "not enabled",
                "not allowed",
            ]
            .iter()
            .any(|predicate| {
                message.contains(&format!("{name}{suffix} {predicate}"))
                    || message.contains(&format!("{name}{suffix} is {predicate}"))
            })
        });
        passive
            || [
                "does not support",
                "unsupported tool",
                "unknown tool",
                "unknown name",
                "unsupported value",
            ]
            .iter()
            .any(|predicate| message.contains(&format!("{predicate} {name}")))
    })
}

fn limited(retry: SystemTime) -> Failure {
    let mut error = request_fault(
        ErrorCode::Unavailable,
        ModelFailure::RateLimited,
        "model search rate limit reached",
        true,
    );
    error.retry_at = Some(retry);
    error
}

fn interrupted() -> Failure {
    request_fault(
        ErrorCode::Unavailable,
        ModelFailure::InvalidResponse,
        "model search response was interrupted",
        true,
    )
}

#[cfg(test)]
mod tests;
