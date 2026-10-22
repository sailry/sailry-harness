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

mod budget;

fn retryable(error: &AdkError) -> bool {
    error.retry.should_retry && is_retryable_model_error(error)
}

fn scheduled(error: &AdkError, stopped: bool, attempt: u64, limit: u32, committed: bool) -> bool {
    !stopped && attempt <= u64::from(limit) && !committed && retryable(error)
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

    fn max_cost_micro_usd(&self, request: &adk_core::LlmRequest) -> Option<u64> {
        self.inner.max_cost_micro_usd(request)
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
        let ingress = self.ingress.clone();
        let turn = self.turn;
        let session = self.session;
        let stop = self.stop.clone();
        let (mut output, responses) = mpsc::channel(0);
        let producer = async move {
            let budget = budget::Budget::default();
            let budget = &budget;
            let committed = AtomicBool::new(false);
            let committed = &committed;
            let published = AtomicBool::new(false);
            let published = &published;
            let model = inner.as_ref();
            let sender = output.clone();
            let policy = RetryConfig::default()
                .with_max_retries(5)
                .with_max_delay(std::time::Duration::from_secs(60));
            let retry_limit = policy.max_retries;
            let limit = u64::from(retry_limit);
            let node = ingress.node;
            let mut index = 0_u64;
            let mut operation = || {
                index += 1;
                let attempt_number = index;
                let started = std::time::Instant::now();
                let stopped = stop.clone();
                let attempt = attempt(
                    model,
                    request.clone(),
                    stream,
                    committed,
                    published,
                    sender.clone(),
                );
                async move {
                    let mut result = attempt.await;
                    if let Err(error) = &mut result {
                        if !budget.schedule(
                            error,
                            stopped.is_cancelled(),
                            attempt_number,
                            retry_limit,
                            committed.load(Ordering::Relaxed),
                            published.load(Ordering::Relaxed),
                        ) {
                            return result;
                        }
                        let mut entry = crate::logging::Entry::new(
                            crate::logging::Area::Model,
                            crate::logging::Phase::Failed,
                        );
                        entry.operation = Some("model_retry");
                        entry.stage = Some("attempt");
                        entry.attempt = Some(attempt_number);
                        entry.limit = Some(limit);
                        entry.duration_us = u64::try_from(started.elapsed().as_micros()).ok();
                        entry.code = Some(ErrorCode::Unavailable);
                        entry.context = crate::logging::Context {
                            node: Some(node),
                            turn: Some(turn),
                            session: Some(session),
                            model: Some(model.name().into()),
                            ..Default::default()
                        };
                        super::cause(&mut entry, error);
                        entry.emit();
                    }
                    result
                }
            };
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
    committed: &AtomicBool,
    published: &AtomicBool,
    mut output: mpsc::Sender<adk_core::Result<LlmResponse>>,
) -> adk_core::Result<()> {
    let recovering = published.load(Ordering::Relaxed);
    let mut responses = model.generate_content(request, stream).await?;
    let mut content = None::<Content>;
    while let Some(response) = responses.next().await {
        let mut response = response?;
        if response.partial
            && response.content.is_none()
            && response.usage_metadata.is_none()
            && response.error_code.is_none()
            && response.error_message.is_none()
        {
            continue;
        }
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
    if stream && !committed.load(Ordering::Relaxed) {
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
