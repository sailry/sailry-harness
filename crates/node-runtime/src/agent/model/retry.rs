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
        let mut identity = super::call::current().unwrap_or_default();
        identity.node = Some(ingress.node);
        identity.turn = turn;
        identity.session = Some(session);
        identity.model = Some(inner.name().into());
        let identity = super::call::identity(identity);
        let (mut output, responses) = mpsc::channel(0);
        let capture = turn.map(|turn| {
            let capture = observation::Capture::new(
                turn,
                inner.name().into(),
                identity.clone(),
                ingress.agents.timeline(turn),
                ingress.agents.sources(turn),
            );
            ingress.agents.observe_model(turn, capture.clone());
            capture
        });
        let context = identity.clone();
        let producer = async move {
            let mut entry = crate::logging::Entry::new(
                crate::logging::Area::Model,
                crate::logging::Phase::Begin,
            );
            entry.context = context;
            entry.operation = Some("model_recovery");
            entry.stage = Some("session");
            let mut recovery = crate::logging::Span::new(entry).cancel_when(stop.clone());
            let mut budget = budget::Budget::default();
            let committed = AtomicBool::new(false);
            let published = AtomicBool::new(false);
            let result = loop {
                if capture.as_ref().is_some_and(|value| {
                    value
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .empty()
                }) {
                    if let Some(capture) = &capture {
                        if let Err(error) =
                            observation::Capture::persist(capture, &ingress, session).await
                        {
                            break Err(error);
                        }
                    }
                }
                let observer = capture.as_ref().map(observation::Capture::observer);
                let request_attempt = attempt(
                    inner.as_ref(),
                    request.clone(),
                    stream,
                    &committed,
                    &published,
                    output.clone(),
                    capture.clone(),
                );
                let observed = async {
                    match observer {
                        Some(observer) => {
                            adk_core::model_observation::scope(observer, request_attempt).await
                        }
                        None => request_attempt.await,
                    }
                };
                let result = tokio::select! {
                    biased;
                    _ = stop.cancelled() => Err(cancelled()),
                    result = observed => result,
                };
                if let Some(capture) = &capture {
                    if let Err(error) =
                        observation::Capture::persist(capture, &ingress, session).await
                    {
                        break Err(error);
                    }
                }
                let error = match result {
                    Ok(terminal) => {
                        if let Some(response) = terminal {
                            let _ = output.send(Ok(response)).await;
                        }
                        break Ok(());
                    }
                    Err(error) => error,
                };
                if stop.is_cancelled() || committed.load(Ordering::Relaxed) {
                    recovery.entry.recovery_decision = Some(if stop.is_cancelled() {
                        "cancelled"
                    } else {
                        "response_committed"
                    });
                    break Err(error);
                }
                let error = interrupted(error);
                let failure = classification::Failure::new(&error);
                let (attempt, finite, at, delay, fallback) =
                    match budget.next(&failure, limit, persistent) {
                        budget::Decision::Stop(reason) => {
                            recovery.entry.recovery_decision = Some(decision(reason));
                            break Err(terminal(error, reason));
                        }
                        budget::Decision::Exhausted => {
                            if failure.transport != Some(Transport::WebSocket)
                                || !ingress.model_clients.fallback(session)
                            {
                                recovery.entry.recovery_decision = Some("attempts_exhausted");
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
                diagnostic(&recovery.entry, &error, &notice).emit();
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
                let _waiting = turn
                    .and_then(|turn| ingress.agents.timeline(turn))
                    .map(|timeline| timeline.span(crate::agent::observation::Phase::Excluded));
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => break Err(cancelled()),
                    _ = tokio::time::sleep_until(at) => {},
                }
            };
            match result {
                Ok(()) => {
                    recovery.entry.recovery_decision = Some("succeeded");
                    recovery.finish(crate::logging::Phase::Completed, None);
                }
                Err(error) => {
                    classify(&mut recovery.entry, &error);
                    recovery
                        .entry
                        .recovery_decision
                        .get_or_insert(if stop.is_cancelled() {
                            "cancelled"
                        } else {
                            "failed"
                        });
                    super::diagnostics::finish(&mut recovery, &error, stop.is_cancelled());
                    let _ = output.send(Err(error)).await;
                }
            }
        };
        // Poll the producer inside its stream so dropping a turn also drops its
        // request, private recovery buffer, observer and backoff timer.
        let producer =
            futures::stream::once(producer).filter_map(|()| futures::future::ready(None));
        Ok(super::call::stream(
            identity,
            Box::pin(futures::stream::select(responses, producer)),
        ))
    }
}

async fn attempt(
    model: &dyn Llm,
    request: LlmRequest,
    stream: bool,
    committed: &AtomicBool,
    published: &AtomicBool,
    mut output: mpsc::Sender<adk_core::Result<LlmResponse>>,
    capture: Option<Arc<std::sync::Mutex<observation::Capture>>>,
) -> adk_core::Result<Option<LlmResponse>> {
    let recovering = published.load(Ordering::Relaxed);
    let mut responses = model.generate_content(request, stream).await?;
    let received = std::time::Instant::now();
    if let Some(capture) = &capture {
        capture
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .responded(received);
    }
    let mut content = None::<Content>;
    while let Some(response) = responses.next().await {
        let mut response = response?;
        if let Some(capture) = &capture {
            capture
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .response(&response);
        }
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
        // Finalize and durably record the scope before ADK can drop this stream.
        if response.turn_complete {
            return Ok(Some(response));
        }
        output.send(Ok(response)).await.map_err(|_| cancelled())?;
        published.store(true, Ordering::Relaxed);
    }
    if stream && !committed.load(Ordering::Relaxed) {
        return Err(AdkError::new(
            adk_core::ErrorComponent::Model,
            adk_core::ErrorCategory::Unavailable,
            "model.response_incomplete",
            "model response ended before a complete response was received",
        ));
    }
    Ok(None)
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

fn decision(reason: Stop) -> &'static str {
    match reason {
        Stop::AttemptsExhausted => "attempts_exhausted",
        Stop::RateLimited => "rate_limited",
        Stop::ServerOverloaded => "server_overloaded",
        Stop::InvalidRequest => "invalid_request",
        Stop::InvalidPrompt => "invalid_prompt",
        Stop::ContextWindowExceeded => "context_window_exceeded",
        Stop::QuotaExceeded => "quota_exceeded",
        Stop::UsageNotIncluded => "usage_not_included",
        Stop::UsageLimitReached => "usage_limit_reached",
        Stop::ResponseIncomplete => "response_incomplete",
        Stop::FlexUnavailable => "flex_unavailable",
        Stop::PolicyViolation => "policy_violation",
        Stop::LocalFailure => "local_failure",
    }
}

fn classify(entry: &mut crate::logging::Entry, error: &AdkError) {
    let failure = classification::Failure::new(error);
    entry.transport = failure.transport.map(|transport| match transport {
        Transport::Http => "http",
        Transport::WebSocket => "websocket",
    });
    entry.reason = Some(match failure.class {
        classification::Class::Terminal(_) => "terminal",
        classification::Class::Backoff => "backoff",
        classification::Class::Advice(_) => "server_advice",
        classification::Class::Connection => "connection",
    });
    super::cause(entry, error);
}

fn diagnostic(
    base: &crate::logging::Entry,
    error: &AdkError,
    notice: &Retry,
) -> crate::logging::Entry {
    let mut entry = base.clone();
    entry.phase = crate::logging::Phase::Retrying;
    entry.operation = Some("model_retry");
    entry.stage = Some("session");
    entry.attempt = Some(notice.attempt);
    entry.limit = notice.limit;
    entry.wait_ms = notice.delay_ms;
    entry.recovery_decision = Some(if notice.fallback { "fallback" } else { "retry" });
    entry.fallback_reason = notice.fallback.then_some("attempts_exhausted");
    classify(&mut entry, error);
    entry
}

#[cfg(test)]
mod tests;
