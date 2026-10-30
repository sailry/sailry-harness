use super::*;
use crate::mcp_transport::{self, Tracked};
use rmcp::{
    RoleClient,
    model::{CallToolRequestParams, CallToolResponse, ClientConfig},
    service::{
        ClientInitializeError, RunningService, ServiceError, serve_client_with_lifecycle_and_ct,
    },
    transport::{
        StreamableHttpClientTransport,
        streamable_http_client::{
            HttpStatusError, StreamableHttpClientTransportConfig, StreamableHttpError,
        },
    },
};
use serde_json::{Value, json};
use tokio::sync::oneshot;

const DEADLINE: Duration = Duration::from_secs(20);
const CLEANUP_LIMIT: Duration = Duration::from_secs(7);

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
    stop: &CancellationToken,
) -> Result<Vec<search::Result>, Failure> {
    if auth.key.is_none() {
        let endpoint = if endpoint == "https://api.exa.ai/search" {
            "https://mcp.exa.ai/mcp"
        } else {
            endpoint
        };
        return hosted(endpoint, input, stop).await;
    }
    let mut body = json!({
        "query": input.query, "numResults": input.limit, "type": "auto",
        "contents": {"text": {"maxCharacters": 4000}},
    });
    if let Some(freshness) = input.freshness {
        body["startPublishedDate"] =
            json!(filters::since(freshness, chrono::Utc::now()).to_rfc3339());
    }
    if !input.include_domains.is_empty() {
        body["includeDomains"] = json!(input.include_domains);
    }
    if !input.exclude_domains.is_empty() {
        body["excludeDomains"] = json!(input.exclude_domains);
    }
    let request = network::json(
        network::client()?
            .post(endpoint)
            .header("x-api-key", network::key(auth)?)
            .json(&body),
    );
    let response = tokio::select! {
        biased;
        _ = stop.cancelled() => return Err(Failure::cancelled()),
        result = tokio::time::timeout(DEADLINE, request) => result
            .unwrap_or_else(|_| Err(Failure::unavailable("search service deadline exceeded")))?,
    };
    parse(&response)
}

pub(super) fn parse(value: &Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/results", "title", "text", "publishedDate")
}

async fn hosted(
    endpoint: &str,
    input: &search::Request,
    request_stop: &CancellationToken,
) -> Result<Vec<search::Result>, Failure> {
    let worker_stop = CancellationToken::new();
    let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint.to_owned())
        .reinit_on_expired_session(false);
    config.max_sse_event_size = network::MAX_BODY;
    let adapter = StreamableHttpClientTransport::with_client(network::client()?, config);
    let (adapter, closed) = Tracked::new(adapter);
    let mut deadline = Box::pin(tokio::time::sleep(DEADLINE));
    let mut initializing = Box::pin(serve_client_with_lifecycle_and_ct(
        ClientConfig::default(),
        adapter,
        mcp_transport::lifecycle(),
        worker_stop.clone(),
    ));
    let initialized = tokio::select! {
        biased;
        _ = request_stop.cancelled() => None,
        _ = &mut deadline => None,
        result = &mut initializing => Some(result),
    };
    if initialized.is_none() {
        worker_stop.cancel();
    }
    drop(initializing);
    let service = match initialized {
        Some(Ok(service)) => service,
        Some(Err(error)) => {
            let failure = initialize(error);
            return Err(if wait_closed(closed).await {
                failure
            } else {
                failure.with_cleanup_uncertain()
            });
        }
        None => {
            let cleaned = wait_closed(closed).await;
            return if request_stop.is_cancelled() {
                Err(Failure::cancelled_with_cleanup(cleaned))
            } else {
                Err(Failure::deadline_with_cleanup(cleaned))
            };
        }
    };
    let params = CallToolRequestParams::new("web_search_exa").with_arguments(
        json!({"query":filters::query(input),"numResults":input.limit})
            .as_object()
            .expect("search arguments are an object")
            .clone(),
    );
    let mut call = Box::pin(service.call_tool_once(params));
    let response = tokio::select! {
        biased;
        _ = request_stop.cancelled() => None,
        _ = &mut deadline => None,
        result = &mut call => Some(result.map_err(|error| match error {
            ServiceError::TransportSend(error) => transport(error.error.as_ref()),
            ServiceError::TransportClosed => Failure::unavailable("search connection closed"),
            _ => Failure::invalid("search service rejected the tool call"),
        })),
    };
    drop(call);
    let cleaned = close(service, closed).await;
    let Some(response) = response else {
        return if request_stop.is_cancelled() {
            Err(Failure::cancelled_with_cleanup(cleaned))
        } else {
            Err(Failure::deadline_with_cleanup(cleaned))
        };
    };
    let response = match response {
        Ok(response) => response,
        Err(failure) => {
            return Err(if cleaned {
                failure
            } else {
                failure.with_cleanup_uncertain()
            });
        }
    };
    if !cleaned {
        return Err(Failure::cleanup_uncertain());
    }
    let CallToolResponse::Complete(response) = response else {
        return Err(Failure::invalid(
            "search service did not return a final result",
        ));
    };
    if response.is_error == Some(true) {
        return Err(Failure::invalid("search service rejected the query"));
    }
    let mut text = String::new();
    for block in response.content {
        if let Some(part) = block.as_text() {
            if part.text.len() > network::MAX_BODY.saturating_sub(text.len()) {
                return Err(Failure::invalid("search service response is too large"));
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&part.text);
        }
    }
    formatted(&text)
}

async fn close(
    service: RunningService<RoleClient, ClientConfig>,
    closed: oneshot::Receiver<bool>,
) -> bool {
    tokio::time::timeout(CLEANUP_LIMIT, async move {
        let joined = service.cancel().await.is_ok();
        let closed = closed.await.unwrap_or(false);
        joined && closed
    })
    .await
    .unwrap_or(false)
}

async fn wait_closed(closed: oneshot::Receiver<bool>) -> bool {
    matches!(
        tokio::time::timeout(CLEANUP_LIMIT, closed).await,
        Ok(Ok(true))
    )
}

fn initialize(error: ClientInitializeError) -> Failure {
    match error {
        ClientInitializeError::TransportError { error, .. } => transport(error.error.as_ref()),
        ClientInitializeError::ConnectionClosed(_) => {
            Failure::unavailable("search connection closed")
        }
        _ => Failure::invalid("search service initialization failed"),
    }
}

fn transport(error: &(dyn std::error::Error + 'static)) -> Failure {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(status) = error.downcast_ref::<HttpStatusError>() {
            return http_status(status);
        }
        if let Some(http) = error.downcast_ref::<StreamableHttpError<reqwest::Error>>() {
            match http {
                StreamableHttpError::HttpStatus(status) => return http_status(status),
                StreamableHttpError::AuthRequired(_)
                | StreamableHttpError::InsufficientScope(_) => return Failure::unauthorized(),
                StreamableHttpError::Client(error) => {
                    return if error
                        .status()
                        .is_some_and(|status| status == 401 || status == 403)
                    {
                        Failure::unauthorized()
                    } else if error
                        .status()
                        .is_some_and(|status| status.is_client_error())
                    {
                        Failure::invalid("search service rejected the request")
                    } else {
                        Failure::unavailable("search service is unreachable")
                    };
                }
                StreamableHttpError::UnexpectedServerResponse(_)
                | StreamableHttpError::UnexpectedContentType(_)
                | StreamableHttpError::Deserialize(_) => {
                    return Failure::invalid("search service returned an invalid response");
                }
                _ => {}
            }
        }
        current = error.source();
    }
    Failure::unavailable("search service is unavailable")
}

fn http_status(status: &HttpStatusError) -> Failure {
    if status.status == 429 {
        let deadline = cooldown::retry_value(status.retry_after.as_deref(), SystemTime::now());
        Failure {
            fault: cooldown::pending(deadline),
            temporary: true,
            retry_at: Some(deadline),
            uncertain: false,
        }
    } else if matches!(status.status, 401 | 403) {
        Failure::unauthorized()
    } else {
        Failure::unavailable("search service is unavailable")
    }
}

pub(super) fn formatted(text: &str) -> Result<Vec<search::Result>, Failure> {
    if text.trim() == "No search results found. Please try a different query." {
        return Ok(Vec::new());
    }
    let entries: Result<Vec<_>, Failure> = text
        .split("\n\n---\n\n")
        .map(|entry| {
            let mut title = None;
            let mut url = None;
            let mut published = None;
            let mut snippet = String::new();
            let mut content = false;
            for line in entry.lines() {
                if content {
                    if !snippet.is_empty() {
                        snippet.push('\n');
                    }
                    snippet.push_str(line);
                } else if let Some(value) = line.strip_prefix("Title: ") {
                    title = Some(value);
                } else if let Some(value) = line.strip_prefix("URL: ") {
                    url = Some(value);
                } else if let Some(value) = line.strip_prefix("Published: ") {
                    if value != "N/A" {
                        published = Some(value);
                    }
                } else if line == "Highlights:" {
                    content = true;
                } else if let Some(value) = line.strip_prefix("Text: ") {
                    content = true;
                    snippet.push_str(value);
                }
            }
            let (Some(title), Some(url)) = (title, url) else {
                return Err(Failure::invalid(
                    "search service returned an invalid result",
                ));
            };
            Ok(json!({"title":title,"url":url,"text":snippet,"publishedDate":published}))
        })
        .collect();
    parse(&json!({"results":entries?}))
}
