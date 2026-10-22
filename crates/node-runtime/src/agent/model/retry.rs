//! Session recovery follows Codex; raw HTTP retries remain owned by the transport.
//! Only uncommitted attempts can recover; complete responses and tool work are never replayed.
use super::*;
use adk_core::{
    AdkError, Content, Event, LlmRequest, LlmResponse, LlmResponseStream, SchemaAdapter,
};
use adk_session::SessionService;
use async_trait::async_trait;
use futures::{SinkExt, StreamExt, channel::mpsc};
use sailry_protocol::SessionId;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) mod budget;
mod classification;
use sailry_protocol::conversation::recovery::{Retry, Stop, Transport};

pub(super) struct Model {
    pub inner: Arc<dyn Llm>,
    pub ingress: Arc<Ingress>,
    pub turn: Option<TurnId>,
    pub session: SessionId,
    pub stop: CancellationToken,
    pub limit: u64,
    pub persistent: bool,
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
        let limit = self.limit;
        let persistent = self.persistent;
        let (mut output, responses) = mpsc::channel(0);
        let producer = async move {
            let mut budget = budget::Budget::default();
            let committed = AtomicBool::new(false);
            let published = AtomicBool::new(false);
            let result = loop {
                let result = tokio::select! {
                    biased;
                    _ = stop.cancelled() => Err(cancelled()),
                    result = attempt(inner.as_ref(), request.clone(), stream, &committed,
                        &published, output.clone()) => result,
                };
                let Err(error) = result else {
                    break Ok(());
                };
                if stop.is_cancelled() || committed.load(Ordering::Relaxed) {
                    break Err(error);
                }
                let error = interrupted(error);
                let failure = classification::Failure::new(&error);
                let (attempt, finite, at, delay, fallback) =
                    match budget.next(&failure, limit, persistent) {
                        budget::Decision::Stop(reason) => break Err(terminal(error, reason)),
                        budget::Decision::Exhausted => {
                            if failure.transport != Some(Transport::WebSocket)
                                || !ingress.model_clients.fallback(session)
                            {
                                break Err(terminal(error, Stop::AttemptsExhausted));
                            }
                            budget.fallback();
                            (
                                0,
                                Some(limit),
                                failure.retry_at.unwrap_or_else(tokio::time::Instant::now),
                                None,
                                true,
                            )
                        }
                        budget::Decision::Retry {
                            attempt,
                            limit,
                            at,
                            delay,
                        } => (attempt, limit, at, delay, false),
                    };
                let notice = Retry {
                    attempt,
                    fallback,
                    limit: finite,
                    delay_ms: Some(
                        u64::try_from(
                            delay
                                .unwrap_or_else(|| {
                                    at.saturating_duration_since(tokio::time::Instant::now())
                                })
                                .as_millis(),
                        )
                        .unwrap_or(u64::MAX),
                    ),
                    transport: failure.transport,
                    status: failure.status,
                    code: failure.code,
                    error: Some(super::error(&error).message),
                };
                let mut entry = crate::logging::Entry::new(
                    crate::logging::Area::Model,
                    crate::logging::Phase::Failed,
                );
                entry.operation = Some("model_retry");
                entry.stage = Some("session");
                entry.attempt = Some(attempt);
                entry.limit = finite;
                entry.context = crate::logging::Context {
                    node: Some(ingress.node),
                    turn,
                    session: Some(session),
                    model: Some(inner.name().into()),
                    ..Default::default()
                };
                super::cause(&mut entry, &error);
                entry.emit();
                if let Some(turn) = turn {
                    let mut event = Event::new(turn.to_string());
                    event.author = "system".into();
                    event.provider_metadata.insert(
                        "sailry_retry".into(),
                        serde_json::to_string(&notice).expect("retry diagnostic serializes"),
                    );
                    let sessions = ingress.sessions(turn);
                    let session_id = session.to_string();
                    let observed = tokio::select! {
                        biased;
                        _ = stop.cancelled() => Err(cancelled()),
                        result = sessions.append_event(&session_id, event) => result,
                    };
                    if let Err(error) = observed {
                        break Err(error);
                    }
                }
                // Server advice keeps its original deadline. Codex starts local
                // backoff after the recovery notification has been delivered.
                let at = delay.map_or(at, |delay| tokio::time::Instant::now() + delay);
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => break Err(cancelled()),
                    _ = tokio::time::sleep_until(at) => {},
                }
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

fn interrupted(error: AdkError) -> AdkError {
    if error
        .details
        .metadata
        .get("provider_tool_completed")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return error;
    }
    let mut fault = super::error(&error);
    fault.code = ErrorCode::OutcomeUnknown;
    fault.message.push_str(
        "\n\nA provider tool completed before the response ended; the request was not replayed",
    );
    error.with_source(fault)
}

fn terminal(error: AdkError, reason: Stop) -> AdkError {
    let mut fault = super::error(&error);
    if !matches!(fault.code, ErrorCode::Cancelled | ErrorCode::OutcomeUnknown)
        && fault.details.is_none()
    {
        fault.details = Some(sailry_protocol::FaultDetails::ModelRecovery { reason });
    }
    error.with_source(fault)
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
