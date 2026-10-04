//! WebSocket transport for the OpenAI Responses API.
//!
//! Provides a persistent WebSocket connection to `/v1/responses` for
//! lower-latency agentic workflows with tool calling. Reuses the same
//! `LlmResponseStream` type as the HTTP path, converting WebSocket frames
//! into `LlmResponse` items via the existing `responses_convert` logic.
//!
//! Gated behind the `openai-ws` feature flag.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use adk_core::{
    AdkError, Content, ErrorCategory, ErrorComponent, LlmResponse, LlmResponseStream, Part,
};
use async_openai::types::responses::CreateResponse;
use async_stream::try_stream;
use futures::{SinkExt, StreamExt};
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Message,
        http::{Request, Uri},
    },
};

use super::config::OpenAIResponsesConfig;
use super::responses_convert;
use crate::retry::RetryConfig;

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Persistent WebSocket connection to the OpenAI Responses API.
///
/// Maintains a single WebSocket connection that can be reused across multiple
/// `generate_content` calls, reducing per-request connection overhead for
/// latency-sensitive agentic workflows.
///
/// # Example
///
/// ```rust,ignore
/// use adk_model::openai::{OpenAIResponsesConfig, ws_transport::WsTransport};
///
/// let config = OpenAIResponsesConfig::new("sk-...", "o3");
/// let transport = WsTransport::connect(&config).await?;
/// ```
pub struct WsTransport {
    /// The underlying WebSocket stream, shared via Arc for spawned reader tasks.
    ws: Arc<Mutex<WsStream>>,
    /// Connection URL (derived from base_url).
    url: String,
    /// API key for reconnection.
    api_key: String,
    /// Retry configuration for reconnection.
    retry_config: RetryConfig,
    /// Whether the connection is currently active.
    connected: Arc<AtomicBool>,
}

impl WsTransport {
    /// Establish a new WebSocket connection to the OpenAI Responses API.
    ///
    /// The WebSocket URL is derived from the configured base URL by replacing
    /// `https://` with `wss://` and appending `/v1/responses`. The API key is
    /// sent as a bearer token in the initial handshake headers.
    ///
    /// # Errors
    ///
    /// Returns `AdkError` with category `Unavailable` if the connection fails.
    pub async fn connect(config: &OpenAIResponsesConfig) -> Result<Self, AdkError> {
        let base_url = config.base_url.as_deref().unwrap_or("https://api.openai.com/v1");

        let ws_url = derive_ws_url(base_url);

        let ws_stream = establish_connection(&ws_url, &config.api_key).await?;

        Ok(Self {
            ws: Arc::new(Mutex::new(ws_stream)),
            url: ws_url,
            api_key: config.api_key.clone(),
            retry_config: RetryConfig::default(),
            connected: Arc::new(AtomicBool::new(true)),
        })
    }

    /// Set the retry configuration for reconnection attempts.
    #[must_use]
    pub fn with_retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }

    /// Returns whether the WebSocket connection is currently active.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Send a request over the WebSocket and return a response stream.
    ///
    /// Serializes the `CreateResponse` request as JSON and sends it over the
    /// WebSocket. Incoming frames are converted to `LlmResponse` items using
    /// the existing `responses_convert` logic.
    ///
    /// If the connection has dropped, attempts reconnection before sending.
    ///
    /// # Errors
    ///
    /// Returns `AdkError` with category `Unavailable` if sending fails and
    /// reconnection is exhausted.
    pub async fn send_request(
        &self,
        request: CreateResponse,
    ) -> Result<LlmResponseStream, AdkError> {
        // Reconnect if not connected
        if !self.connected.load(Ordering::Relaxed) {
            self.reconnect().await?;
        }

        // Serialize the request
        let request_json = serde_json::to_string(&request).map_err(|e| {
            AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::Internal,
                "model.openai_responses.ws_serialize",
                format!("Failed to serialize WebSocket request: {e}"),
            )
            .with_provider("openai-responses")
        })?;

        // Send the request
        {
            let mut ws = self.ws.lock().await;
            ws.send(Message::Text(request_json.into())).await.map_err(|e| {
                self.connected.store(false, Ordering::Relaxed);
                AdkError::new(
                    ErrorComponent::Model,
                    ErrorCategory::Unavailable,
                    "model.openai_responses.ws_send",
                    format!("Failed to send WebSocket message: {e}"),
                )
                .with_provider("openai-responses")
            })?;
        }

        // Build a stream that reads WebSocket frames and converts them
        let response_stream = self.build_response_stream();

        Ok(response_stream)
    }

    /// Attempt reconnection with exponential backoff using configured retry policy.
    ///
    /// # Errors
    ///
    /// Returns `AdkError` with category `Unavailable` and code
    /// `model.openai_responses.ws_reconnect_exhausted` if all retries fail.
    async fn reconnect(&self) -> Result<(), AdkError> {
        if !self.retry_config.enabled {
            return Err(AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::Unavailable,
                "model.openai_responses.ws_reconnect_exhausted",
                "WebSocket reconnection disabled by retry config",
            )
            .with_provider("openai-responses"));
        }

        let mut delay = self.retry_config.initial_delay;

        for attempt in 0..self.retry_config.max_retries {
            tracing::info!(
                attempt = attempt + 1,
                max_retries = self.retry_config.max_retries,
                "attempting WebSocket reconnection"
            );

            match establish_connection(&self.url, &self.api_key).await {
                Ok(new_stream) => {
                    let mut ws = self.ws.lock().await;
                    *ws = new_stream;
                    self.connected.store(true, Ordering::Relaxed);
                    tracing::info!("WebSocket reconnection successful");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(
                        attempt = attempt + 1,
                        error = %e,
                        "WebSocket reconnection attempt failed"
                    );

                    if attempt + 1 < self.retry_config.max_retries {
                        tokio::time::sleep(delay).await;
                        delay = std::cmp::min(
                            std::time::Duration::from_secs_f64(
                                delay.as_secs_f64()
                                    * f64::from(self.retry_config.backoff_multiplier),
                            ),
                            self.retry_config.max_delay,
                        );
                    }
                }
            }
        }

        Err(AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Unavailable,
            "model.openai_responses.ws_reconnect_exhausted",
            format!(
                "WebSocket reconnection failed after {} attempts",
                self.retry_config.max_retries
            ),
        )
        .with_provider("openai-responses"))
    }

    /// Build a response stream that reads WebSocket frames and converts them
    /// to `LlmResponse` items using the same logic as the HTTP SSE path.
    ///
    /// Spawns a background task that reads frames from the WebSocket and
    /// forwards parsed responses through an mpsc channel.
    fn build_response_stream(&self) -> LlmResponseStream {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<LlmResponse, AdkError>>(64);

        let ws = Arc::clone(&self.ws);
        let connected = Arc::clone(&self.connected);

        tokio::spawn(async move {
            loop {
                let frame = {
                    let mut guard = ws.lock().await;
                    guard.next().await
                };

                match frame {
                    Some(Ok(Message::Text(text))) => {
                        match parse_ws_event(&text) {
                            Some(WsEvent::TextDelta(delta)) => {
                                let response = LlmResponse {
                                    content: Some(Content {
                                        role: "model".to_string(),
                                        parts: vec![Part::Text { text: delta }],
                                    }),
                                    partial: true,
                                    turn_complete: false,
                                    ..Default::default()
                                };
                                if tx.send(Ok(response)).await.is_err() {
                                    break;
                                }
                            }
                            Some(WsEvent::ReasoningDelta(delta)) => {
                                let response = LlmResponse {
                                    content: Some(Content {
                                        role: "model".to_string(),
                                        parts: vec![Part::Thinking {
                                            thinking: delta,
                                            signature: None,
                                        }],
                                    }),
                                    partial: true,
                                    turn_complete: false,
                                    ..Default::default()
                                };
                                if tx.send(Ok(response)).await.is_err() {
                                    break;
                                }
                            }
                            Some(WsEvent::Completed(response_json)) => {
                                let response = parse_completed_response(&response_json);
                                let _ = tx.send(Ok(response)).await;
                                break;
                            }
                            Some(WsEvent::Failed { code, message }) => {
                                let response = LlmResponse {
                                    error_code: Some(code),
                                    error_message: Some(message),
                                    turn_complete: true,
                                    ..Default::default()
                                };
                                let _ = tx.send(Ok(response)).await;
                                break;
                            }
                            Some(WsEvent::Error { code, message }) => {
                                let response = LlmResponse {
                                    error_code: Some(code),
                                    error_message: Some(message),
                                    turn_complete: true,
                                    ..Default::default()
                                };
                                let _ = tx.send(Ok(response)).await;
                                break;
                            }
                            None => {
                                // Unknown event type, skip
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        connected.store(false, Ordering::Relaxed);
                        break;
                    }
                    Some(Err(e)) => {
                        connected.store(false, Ordering::Relaxed);
                        let err = AdkError::new(
                            ErrorComponent::Model,
                            ErrorCategory::Unavailable,
                            "model.openai_responses.ws_read",
                            format!("WebSocket read error: {e}"),
                        )
                        .with_provider("openai-responses");
                        let _ = tx.send(Err(err)).await;
                        break;
                    }
                    None => {
                        connected.store(false, Ordering::Relaxed);
                        break;
                    }
                    _ => {
                        // Binary or other frame types — skip
                    }
                }
            }
        });

        let stream = try_stream! {
            while let Some(result) = rx.recv().await {
                match result {
                    Ok(response) => yield response,
                    Err(e) => Err(e)?,
                }
            }
        };

        Box::pin(stream)
    }
}

/// Derive the WebSocket URL from the base HTTP URL.
///
/// Replaces `https://` with `wss://` (or `http://` with `ws://`) and
/// ensures the path ends with `/responses`.
fn derive_ws_url(base_url: &str) -> String {
    let ws_base = if base_url.starts_with("https://") {
        base_url.replacen("https://", "wss://", 1)
    } else if base_url.starts_with("http://") {
        base_url.replacen("http://", "ws://", 1)
    } else {
        format!("wss://{base_url}")
    };

    // Strip trailing slash and ensure /responses path
    let trimmed = ws_base.trim_end_matches('/');
    if trimmed.ends_with("/responses") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/responses")
    } else {
        format!("{trimmed}/v1/responses")
    }
}

/// Establish a WebSocket connection with bearer token authentication.
async fn establish_connection(url: &str, api_key: &str) -> Result<WsStream, AdkError> {
    let uri: Uri = url.parse().map_err(|e| {
        AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::InvalidInput,
            "model.openai_responses.ws_invalid_url",
            format!("Invalid WebSocket URL '{url}': {e}"),
        )
        .with_provider("openai-responses")
    })?;

    let host = uri.host().unwrap_or("api.openai.com").to_string();

    let request = Request::builder()
        .uri(url)
        .header("Host", &host)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Sec-WebSocket-Key", generate_ws_key())
        .header("Sec-WebSocket-Version", "13")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .body(())
        .map_err(|e| {
            AdkError::new(
                ErrorComponent::Model,
                ErrorCategory::Internal,
                "model.openai_responses.ws_request_build",
                format!("Failed to build WebSocket request: {e}"),
            )
            .with_provider("openai-responses")
        })?;

    let (ws_stream, _response) = connect_async(request).await.map_err(|e| {
        AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Unavailable,
            "model.openai_responses.ws_connect",
            format!("WebSocket connection failed: {e}"),
        )
        .with_provider("openai-responses")
    })?;

    Ok(ws_stream)
}

/// Generate a random WebSocket key for the handshake.
fn generate_ws_key() -> String {
    use base64::Engine;
    let mut key = [0u8; 16];
    // Use a simple counter-based approach for the key since it just needs to be unique
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let bytes = now.as_nanos().to_le_bytes();
    key[..16].copy_from_slice(&bytes[..16]);
    base64::engine::general_purpose::STANDARD.encode(key)
}

/// Parsed WebSocket event types from the OpenAI Responses API.
enum WsEvent {
    /// Text content delta.
    TextDelta(String),
    /// Reasoning summary delta.
    ReasoningDelta(String),
    /// Response completed with full response JSON.
    Completed(serde_json::Value),
    /// Response failed.
    Failed { code: String, message: String },
    /// Response error event.
    Error { code: String, message: String },
}

/// Parse a WebSocket text frame into a typed event.
///
/// The OpenAI WebSocket API sends JSON events with a `type` field
/// matching the SSE event types.
fn parse_ws_event(text: &str) -> Option<WsEvent> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let event_type = value.get("type")?.as_str()?;

    match event_type {
        "response.output_text.delta" => {
            let delta = value.get("delta")?.as_str()?.to_string();
            Some(WsEvent::TextDelta(delta))
        }
        "response.reasoning_summary_text.delta" => {
            let delta = value.get("delta")?.as_str()?.to_string();
            Some(WsEvent::ReasoningDelta(delta))
        }
        "response.completed" => {
            let response = value.get("response")?.clone();
            Some(WsEvent::Completed(response))
        }
        "response.failed" => {
            let response = value.get("response")?;
            let error = response.get("error")?;
            let code = error.get("code").and_then(|c| c.as_str()).unwrap_or("unknown").to_string();
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Response failed")
                .to_string();
            Some(WsEvent::Failed { code, message })
        }
        "response.error" | "error" => {
            let code = value.get("code").and_then(|c| c.as_str()).unwrap_or("error").to_string();
            let message = value
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            Some(WsEvent::Error { code, message })
        }
        _ => None,
    }
}

/// Parse a completed response JSON into an `LlmResponse` using the
/// existing `responses_convert` logic.
fn parse_completed_response(response_json: &serde_json::Value) -> LlmResponse {
    // Attempt to deserialize into the async-openai Response type
    match serde_json::from_value::<async_openai::types::responses::Response>(response_json.clone())
    {
        Ok(response) => {
            let mut adk_response = responses_convert::from_response(&response);
            adk_response.turn_complete = true;
            adk_response.partial = false;
            adk_response
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to parse WebSocket completed response, returning raw");
            // Fallback: return a minimal response with the raw JSON in provider_metadata
            LlmResponse {
                provider_metadata: Some(serde_json::json!({
                    "openai": {
                        "raw_response": response_json,
                        "parse_error": e.to_string(),
                    }
                })),
                turn_complete: true,
                partial: false,
                ..Default::default()
            }
        }
    }
}
