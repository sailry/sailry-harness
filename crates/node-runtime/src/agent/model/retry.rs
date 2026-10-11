//! ADK owns backoff and error classification; retry notices use canonical session events.
//! Only uncommitted attempts can recover; complete responses and tool work are never replayed.
use super::*;
use adk_core::{
    AdkError, Content, Event, LlmRequest, LlmResponse, LlmResponseStream, SchemaAdapter,
};
use adk_model::retry::{execute_with_retry_observer, is_retryable_model_error};
use adk_session::SessionService;
use async_trait::async_trait;
use futures::{SinkExt, StreamExt, channel::mpsc};
use sailry_protocol::SessionId;
use std::sync::atomic::{AtomicBool, Ordering};

// ADK emits model content, not every transport heartbeat. Reasoning and complete
// tool arguments may arrive minutes after the provider has accepted the request.
const RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

fn retryable(error: &AdkError) -> bool {
    // Expiring our own wait does not prove that the provider stopped generating.
    error.code != "model.response_timeout" && is_retryable_model_error(error)
}

async fn open(
    model: &dyn Llm,
    request: LlmRequest,
    stream: bool,
) -> adk_core::Result<LlmResponseStream> {
    tokio::time::timeout(RESPONSE_TIMEOUT, async {
        let mut responses = model.generate_content(request, stream).await?;
        // Include deferred HTTP requests and the first model output in the deadline.
        let first = responses.next().await.transpose()?;
        Ok(responsive(Box::pin(
            futures::stream::iter(first.map(Ok)).chain(responses),
        )))
    })
    .await
    .map_err(|_| timeout())?
}

fn timeout() -> AdkError {
    AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::Timeout,
        "model.response_timeout",
        "model stream was inactive for 5 minutes; the request was not replayed",
    )
}

fn responsive(responses: LlmResponseStream) -> LlmResponseStream {
    // Reset on each ADK event, including lifecycle/tool progress with no text.
    // Empty activity events keep the stream alive without becoming chat entries.
    Box::pin(
        futures::stream::unfold(Some(responses), |state| async move {
            let mut responses = state?;
            match tokio::time::timeout(RESPONSE_TIMEOUT, responses.next()).await {
                Ok(Some(result)) => Some((result, Some(responses))),
                Ok(None) => None,
                Err(_) => Some((Err(timeout()), None)),
            }
        })
        .filter_map(|result| {
            futures::future::ready(match result {
                Ok(response)
                    if response.partial
                        && response.content.is_none()
                        && response.usage_metadata.is_none()
                        && response.error_code.is_none()
                        && response.error_message.is_none() =>
                {
                    None
                }
                result => Some(result),
            })
        }),
    )
}

pub(super) struct Model {
    pub inner: Arc<dyn Llm>,
    pub api: ModelApi,
    pub ingress: Arc<Ingress>,
    pub turn: TurnId,
    pub session: SessionId,
    pub stop: CancellationToken,
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn schema_adapter(&self) -> &dyn SchemaAdapter {
        self.inner.schema_adapter()
    }
    fn uses_interactions_api(&self) -> bool {
        self.inner.uses_interactions_api()
    }

    async fn generate_content(
        &self,
        request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        if self.stop.is_cancelled() {
            return Err(cancelled());
        }
        let inner = self.inner.clone();
        let api = self.api;
        let ingress = self.ingress.clone();
        let turn = self.turn;
        let session = self.session;
        let stop = self.stop.clone();
        let (mut output, responses) = mpsc::channel(0);
        let producer = async move {
            let committed = AtomicBool::new(false);
            let committed = &committed;
            let published = AtomicBool::new(false);
            let published = &published;
            let model = inner.as_ref();
            let sender = output.clone();
            let mut operation = || {
                attempt(
                    model,
                    request.clone(),
                    stream,
                    api,
                    committed,
                    published,
                    sender.clone(),
                )
            };
            let policy = RetryConfig::default().with_max_retries(5);
            let result = tokio::select! {
                biased;
                _ = stop.cancelled() => Err(cancelled()),
                result = execute_with_retry_observer(
                    &policy,
                    |error| !committed.load(Ordering::Relaxed) && retryable(error),
                    None,
                    &mut operation,
                    |attempt, limit, _| {
                        let ingress = &ingress;
                        async move {
                            let mut event = Event::new(turn.to_string());
                            event.author = "system".into();
                            event.provider_metadata.insert(
                                "sailry_retry".into(),
                                serde_json::json!({"attempt": attempt, "limit": limit}).to_string(),
                            );
                            ingress.sessions(turn)
                                .append_event(&session.to_string(), event)
                                .await
                        }
                    },
                ) => result,
            };
            if let Err(error) = result {
                let _ = output.send(Err(error)).await;
            }
        };
        // Poll the producer inside its stream so dropping a turn also drops its
        // request, private recovery buffer, observer and backoff timer.
        let producer =
            futures::stream::once(producer).filter_map(|()| futures::future::ready(None));
        Ok(Box::pin(futures::stream::select(responses, producer)))
    }
}

async fn attempt(
    model: &dyn Llm,
    request: LlmRequest,
    stream: bool,
    api: ModelApi,
    committed: &AtomicBool,
    published: &AtomicBool,
    mut output: mpsc::Sender<adk_core::Result<LlmResponse>>,
) -> adk_core::Result<()> {
    let recovering = published.load(Ordering::Relaxed);
    let mut responses = open(model, request, stream).await?;
    let mut content = None::<Content>;
    while let Some(response) = responses.next().await {
        let mut response = response?;
        if recovering && !committed.load(Ordering::Relaxed) {
            if let Some(next) = response.content.take() {
                let complete = !response.partial
                    && response
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("content_complete"))
                        .and_then(serde_json::Value::as_bool)
                        == Some(true);
                if let Some(current) = &mut content
                    && !complete
                {
                    current.parts.extend(next.parts);
                } else {
                    content = Some(next);
                }
            }
            if response.partial {
                continue;
            }
            // Replace the first attempt's published draft in ADK and Client.
            // An empty successful response must also clear that draft.
            response.content = Some(content.take().unwrap_or_else(|| Content::new("model")));
            let metadata = response
                .provider_metadata
                .get_or_insert_with(|| serde_json::json!({}));
            if !metadata.is_object() {
                *metadata = serde_json::json!({});
            }
            metadata["content_complete"] = true.into();
        }
        if !response.partial {
            committed.store(true, Ordering::Relaxed);
        }
        let complete = response.turn_complete;
        output.send(Ok(response)).await.map_err(|_| cancelled())?;
        published.store(true, Ordering::Relaxed);
        if complete {
            return Ok(());
        }
    }
    if stream && api == ModelApi::Responses && !committed.load(Ordering::Relaxed) {
        return Err(AdkError::new(
            adk_core::ErrorComponent::Model,
            adk_core::ErrorCategory::Unavailable,
            "model.response_incomplete",
            "model response ended before a complete response was received",
        ));
    }
    Ok(())
}

fn cancelled() -> AdkError {
    AdkError::new(
        adk_core::ErrorComponent::Model,
        adk_core::ErrorCategory::Cancelled,
        "model.request_cancelled",
        "model request cancelled",
    )
}

#[cfg(test)]
mod tests;
