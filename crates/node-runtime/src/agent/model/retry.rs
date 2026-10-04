//! ADK owns backoff and error classification; retry notices use canonical session events.
//! Only opening a model response is retried. Published output and tool work are never replayed.
use super::*;
use adk_core::{AdkError, Event, LlmRequest, LlmResponseStream, SchemaAdapter};
use adk_model::retry::{execute_with_retry_observer, is_retryable_model_error};
use adk_session::SessionService;
use async_trait::async_trait;
use futures::StreamExt;
use sailry_protocol::SessionId;

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
        let mut open = || open(self.inner.as_ref(), request.clone(), stream);
        let policy = RetryConfig::default().with_max_retries(5);
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(AdkError::model("model request cancelled")),
            result = execute_with_retry_observer(&policy, retryable, None, &mut open,
                |attempt, limit, _| async move {
                    let mut event = Event::new(self.turn.to_string());
                    event.author = "system".into();
                    event.provider_metadata.insert("sailry_retry".into(),
                        serde_json::json!({"attempt":attempt,"limit":limit}).to_string());
                    self.ingress.sessions(self.turn).append_event(&self.session.to_string(), event).await
                }) => result,
        }
    }
}

#[cfg(test)]
mod tests;
