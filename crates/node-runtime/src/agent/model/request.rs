//! Observe one GDK call without inferring its private HTTP retries or headers.
use super::*;
use crate::agent::{canonical, observation::Phase};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use goose_provider_types::{
    base::{MessageStream, Provider as GdkProvider},
    conversation::{
        message::{Message, MessageContentBlock},
        token_usage::ProviderUsage,
    },
    errors::ProviderError,
    model::ModelConfig,
};
use sailry_protocol::{
    SessionId,
    conversation::{
        Usage,
        observation::{Attempt, Outcome, Request},
    },
};
use std::time::Instant;

pub(super) struct Model {
    inner: Arc<dyn GdkProvider>,
    ingress: Arc<Ingress>,
    turn: TurnId,
    session: SessionId,
    purpose: Purpose,
    stop: CancellationToken,
    identity: crate::logging::Context,
    sources: observation::Sources,
    names: names::Registry,
    metric: Option<crate::telemetry::metrics::Model>,
}

impl Model {
    pub(super) fn wrap(
        inner: Arc<dyn GdkProvider>,
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        purpose: Purpose,
        stop: &CancellationToken,
        sources: observation::Sources,
        names: names::Registry,
    ) -> Arc<dyn GdkProvider> {
        observation::install();
        Arc::new(Self {
            inner,
            ingress: ingress.clone(),
            turn: invocation.turn.id,
            session: invocation.turn.session,
            purpose,
            stop: stop.clone(),
            sources,
            names,
            metric: invocation
                .provider
                .as_ref()
                .filter(|_| crate::telemetry::metrics_enabled(ingress.profile()))
                .map(|provider| {
                    crate::telemetry::metrics::Model::capture(
                        provider,
                        &invocation.turn.config.model,
                    )
                }),
            identity: crate::logging::Context {
                node: Some(ingress.node),
                turn: Some(invocation.turn.id),
                session: Some(invocation.turn.session),
                worktree: Some(invocation.worktree),
                provider: invocation.provider.as_ref().map(|provider| provider.id),
                provider_name: invocation
                    .provider
                    .as_ref()
                    .map(|provider| provider.name.clone()),
                model: Some(invocation.turn.config.model.clone()),
                ..Default::default()
            },
        })
    }
}

#[async_trait]
impl GdkProvider for Model {
    fn get_name(&self) -> &str {
        self.inner.get_name()
    }
    fn manages_own_context(&self) -> bool {
        self.inner.manages_own_context()
    }
    async fn get_context_limit(&self, model: &str, limit: Option<usize>) -> usize {
        self.inner.get_context_limit(model, limit).await
    }
    async fn stream(
        &self,
        config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[rmcp::model::Tool],
    ) -> Result<MessageStream, ProviderError> {
        let exposure = self.names.get(self.purpose)?;
        let messages = exposure.messages(messages);
        let tools = exposure.tools(tools);
        let mut capture = Capture::new(self, config, system)?;
        let payload = capture
            .state
            .as_ref()
            .expect("active request")
            .payload
            .clone();
        let response = payload
            .scope(self.inner.stream(config, system, &messages, &tools))
            .await;
        let responses = match response {
            Ok(responses) => responses,
            Err(error) => {
                capture.record_failure(&error).await;
                return Err(error);
            }
        };
        Ok(Box::pin(stream::unfold(
            Some((responses, capture, exposure)),
            |state| async move {
                let (mut responses, mut capture, exposure) = state?;
                let payload = capture
                    .state
                    .as_ref()
                    .expect("active request")
                    .payload
                    .clone();
                let response = payload.scope(responses.next()).await;
                match response {
                    Some(Ok((mut message, mut usage))) => {
                        if let Some(message) = &mut message {
                            exposure.restore(message);
                        }
                        capture.receive(message.as_ref(), usage.as_mut());
                        Some((Ok((message, usage)), Some((responses, capture, exposure))))
                    }
                    Some(Err(error)) => {
                        capture.record_failure(&error).await;
                        Some((Err(error), None))
                    }
                    None => match capture.finish(Outcome::Succeeded, None).await {
                        Ok(()) => None,
                        Err(error) => Some((Err(error), None)),
                    },
                }
            },
        )))
    }
}

struct Capture {
    state: Option<State>,
}
struct State {
    ingress: Arc<Ingress>,
    session: SessionId,
    request: Request,
    purpose: Purpose,
    started: Instant,
    first_text: Option<u64>,
    usage: Option<Usage>,
    tokens: crate::telemetry::metrics::Tokens,
    metric: Option<(crate::telemetry::Capture, crate::telemetry::metrics::Model)>,
    cache_reported: bool,
    reasoning_reported: bool,
    payload: observation::Payload,
    stop: CancellationToken,
    lease: Lease,
    span: crate::logging::Span,
}
struct Lease {
    controls: Arc<crate::agent::Controls>,
    turn: TurnId,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.controls.finish_operation(self.turn);
    }
}
impl Capture {
    async fn record_failure(&mut self, error: &ProviderError) {
        if let Err(recording) = self.finish(Outcome::Failed, Some(error)).await {
            tracing::error!(error = %recording, "model request observation could not be recorded");
        }
    }

    fn new(model: &Model, config: &ModelConfig, system: &str) -> Result<Self, ProviderError> {
        model
            .ingress
            .agents
            .begin_operation(model.turn)
            .ok_or_else(|| {
                ProviderError::RequestFailed("turn is no longer accepting model requests".into())
            })?;
        let mut identity = model.identity.clone();
        identity.call = Some(sailry_protocol::RequestId::new());
        let started = Instant::now();
        let mut entry =
            crate::logging::Entry::new(crate::logging::Area::Model, crate::logging::Phase::Begin);
        entry.context = identity.clone();
        entry.operation = Some(match model.purpose {
            Purpose::Compaction => "compaction",
            Purpose::Main => "generate",
        });
        entry.stage = Some("provider");
        entry.stream = Some(true);
        entry.budget = config.max_tokens.map(i64::from);
        let span = crate::logging::Span::begin(entry, started).cancel_when(model.stop.clone());
        Ok(Self {
            state: Some(State {
                ingress: model.ingress.clone(),
                session: model.session,
                request: Request {
                    id: identity.call.unwrap().to_string(),
                    turn: model.turn,
                    turn_index: None,
                    model: config.model_name.clone(),
                    context_limit: config
                        .context_limit
                        .and_then(|value| u32::try_from(value).ok()),
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    boundary: None,
                    attempts: Vec::new(),
                },
                purpose: model.purpose,
                started,
                first_text: None,
                usage: None,
                tokens: Default::default(),
                metric: model.metric.clone().and_then(|metric| {
                    crate::telemetry::capture(model.ingress.profile())
                        .map(|capture| (capture, metric))
                }),
                cache_reported: false,
                reasoning_reported: false,
                payload: model
                    .sources
                    .capture(system, started)
                    .with_identity(identity),
                stop: model.stop.clone(),
                lease: Lease {
                    controls: model.ingress.agents.clone(),
                    turn: model.turn,
                },
                span,
            }),
        })
    }
    fn receive(&mut self, message: Option<&Message>, usage: Option<&mut ProviderUsage>) {
        let state = self.state.as_mut().expect("active request");
        if state.first_text.is_none() && message.is_some_and(|message| message.content.iter().any(|content| matches!(content, MessageContentBlock::Text(text) if !text.text.is_empty()))) {
            state.first_text = Some(micros(state.started.elapsed()));
        }
        if let Some(usage) = usage {
            let stats = usage.stats.get_or_insert_with(Default::default);
            stats
                .elapsed_ms
                .get_or_insert_with(|| millis(state.started.elapsed()));
            if stats.time_to_first_token_ms.is_none() {
                stats.time_to_first_token_ms = state.first_text.map(|value| value / 1000);
            }
            state.cache_reported = usage.usage.cache_read_input_tokens.is_some();
            if let Ok(usage) = crate::agent::engine::history::provider_usage(usage) {
                state.tokens = crate::telemetry::metrics::Tokens {
                    input: usage.input.unwrap_or_default(),
                    output: usage.output.unwrap_or_default(),
                    cache_read: usage.cache_read.unwrap_or_default(),
                    cache_write: usage.cache_write.unwrap_or_default(),
                };
                state.reasoning_reported = usage.reasoning.is_some();
                state.usage = counts(&usage);
            }
        }
    }
    async fn finish(
        &mut self,
        outcome: Outcome,
        error: Option<&ProviderError>,
    ) -> Result<(), ProviderError> {
        if let Some(state) = self.state.take() {
            let error = error.cloned();
            // A cancelled caller must not cancel the already-owned history append.
            tokio::spawn(async move { state.persist(outcome, error.as_ref()).await })
                .await
                .map_err(|_| {
                    ProviderError::ExecutionError(
                        "model request observation could not be recorded".into(),
                    )
                })?
        } else {
            Ok(())
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            // The existing turn lease keeps settlement behind this same history append.
            tokio::spawn(async move {
                if let Err(error) = state.persist(Outcome::Cancelled, None).await {
                    tracing::error!(error = %error, "model request observation could not be recorded");
                }
            });
        }
    }
}
impl State {
    async fn persist(
        mut self,
        outcome: Outcome,
        error: Option<&ProviderError>,
    ) -> Result<(), ProviderError> {
        let elapsed = self.started.elapsed();
        let outcome = if self.stop.is_cancelled() {
            Outcome::Cancelled
        } else {
            outcome
        };
        // Package HTTP reports its latest prepared request; native HTTP stays opaque.
        let (categories, complete_context, http) = self.payload.snapshot();
        self.request.attempts.push(Attempt {
            index: 1,
            transport: "gdk".into(),
            reused: None,
            headers_us: http.map(|receipt| receipt.headers_us),
            response_us: http.map(|receipt| receipt.headers_us),
            first_text_us: self.first_text,
            elapsed_us: Some(micros(elapsed)),
            status: http.map(|receipt| receipt.status),
            outcome,
            categories,
            complete_context,
            usage: self.usage,
            cache_reported: self.cache_reported,
            reasoning_reported: self.reasoning_reported,
        });
        if let Some(timeline) = self.ingress.agents.timeline(self.request.turn) {
            let wait = self.first_text.unwrap_or_else(|| micros(elapsed));
            timeline.record(
                self.started,
                std::time::Duration::from_micros(wait),
                Phase::Waiting,
            );
            if wait < micros(elapsed) {
                timeline.record(
                    self.started + std::time::Duration::from_micros(wait),
                    std::time::Duration::from_micros(micros(elapsed) - wait),
                    Phase::Output,
                );
            }
        }
        self.span.entry.stage = Some("provider");
        self.span.entry.first_token_us = self.first_text;
        let code = if outcome == Outcome::Cancelled {
            Some(ErrorCode::Cancelled)
        } else {
            error.map(|error| super::errors::fault(error.clone()).code)
        };
        if let Some((capture, model)) = self.metric.take() {
            use crate::telemetry::metrics::{Observation, RequestResult, StatusClass};
            capture.record(Observation::Model {
                model,
                result: match outcome {
                    Outcome::Succeeded => RequestResult::Success,
                    Outcome::Failed => RequestResult::Error,
                    Outcome::Cancelled => RequestResult::Cancelled,
                },
                status: StatusClass::from(http.map(|receipt| receipt.status)),
                error: code,
                // GDK does not expose its private HTTP retry count.
                retries: None,
                tokens: self.tokens,
            });
        }
        self.span.finish(
            match outcome {
                Outcome::Succeeded => crate::logging::Phase::Completed,
                Outcome::Failed => crate::logging::Phase::Failed,
                Outcome::Cancelled => crate::logging::Phase::Cancelled,
            },
            code,
        );
        let record = canonical::Record {
            id: format!("model:{}:request", self.request.id),
            created: chrono::Utc::now(),
            turn: self.request.turn,
            provenance: canonical::Provenance {
                author: "system".into(),
                branch: String::new(),
            },
            metadata: canonical::Metadata {
                execution: Some(canonical::Execution {
                    purpose: Some(match self.purpose {
                        Purpose::Compaction => canonical::Purpose::Compaction,
                        _ => canonical::Purpose::Main,
                    }),
                    observation: Some(self.request),
                    ..Default::default()
                }),
                runtime: Some(canonical::Runtime {
                    user_visible: false,
                    agent_visible: false,
                    ..Default::default()
                }),
                ..Default::default()
            },
            kind: canonical::Kind::Message(canonical::Message::default()),
        };
        let result = self
            .ingress
            .sessions(record.turn)
            .append_event(&self.session.to_string(), record)
            .await;
        // Explicitly retain the lease through the storage acknowledgement.
        drop(self.lease);
        result.map_err(|_| {
            ProviderError::ExecutionError("model request observation could not be recorded".into())
        })
    }
}
fn counts(usage: &canonical::Usage) -> Option<Usage> {
    Some(Usage {
        input: usage.input?,
        output: usage.output?,
        cached_input: usage.cache_read.unwrap_or(0),
        reasoning: usage.reasoning.unwrap_or(0),
    })
}
fn micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}
fn millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
