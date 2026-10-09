use super::*;
use rmcp::{
    model::{CallToolRequestParams, CallToolResponse, ClientInfo},
    service::{ClientInitializeError, ServiceError},
    transport::{
        StreamableHttpClientTransport,
        streamable_http_client::{
            HttpStatusError, StreamableHttpClientTransportConfig, StreamableHttpError,
        },
    },
};
use serde_json::{Value, json};

pub(super) async fn search(
    endpoint: &str,
    auth: &Auth,
    input: &search::Request,
) -> Result<Vec<search::Result>, Failure> {
    if auth.key.is_none() {
        let endpoint = if endpoint == "https://api.exa.ai/search" {
            "https://mcp.exa.ai/mcp"
        } else {
            endpoint
        };
        return hosted(endpoint, input).await;
    }
    let response = network::json(network::client()?.post(endpoint).header("x-api-key", network::key(auth)?).json(&json!({
        "query": input.query, "numResults": input.limit, "type": "auto", "contents": {"text": {"maxCharacters": 4000}},
    }))).await?;
    parse(&response)
}

pub(super) fn parse(value: &Value) -> Result<Vec<search::Result>, Failure> {
    network::results(value, "/results", "title", "text", "publishedDate")
}

async fn hosted(endpoint: &str, input: &search::Request) -> Result<Vec<search::Result>, Failure> {
    let stop = CancellationToken::new();
    let _guard = stop.clone().drop_guard();
    let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint.to_owned())
        .reinit_on_expired_session(false);
    config.max_sse_event_size = network::MAX_BODY;
    let adapter = StreamableHttpClientTransport::with_client(network::client()?, config);
    let service = rmcp::service::serve_client_with_ct(ClientInfo::default(), adapter, stop)
        .await
        .map_err(initialize)?;
    let params = CallToolRequestParams::new("web_search_exa").with_arguments(
        json!({"query":input.query,"numResults":input.limit})
            .as_object()
            .expect("search arguments are an object")
            .clone(),
    );
    let response = service
        .call_tool_once(params)
        .await
        .map_err(|error| match error {
            ServiceError::TransportSend(error) => transport(error.error.as_ref()),
            ServiceError::TransportClosed => Failure::unavailable("search connection closed"),
            _ => Failure::invalid("search service rejected the tool call"),
        });
    let _ = service.cancel().await;
    let CallToolResponse::Complete(response) = response? else {
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
        }
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
        .take(search::MAX_LIMIT as usize)
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
