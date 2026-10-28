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
}

impl Model {
    pub(super) fn wrap(
        inner: Arc<dyn GdkProvider>,
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        purpose: Purpose,
        stop: &CancellationToken,
    ) -> Arc<dyn GdkProvider> {
        Arc::new(Self {
            inner,
            ingress: ingress.clone(),
            turn: invocation.turn.id,
            session: invocation.turn.session,
            purpose,
            stop: stop.clone(),
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
        let mut capture = Capture::new(self, config)?;
        let response = self.inner.stream(config, system, messages, tools).await;
        let responses = match response {
            Ok(responses) => responses,
            Err(error) => {
                capture.finish(Outcome::Failed, Some(&error)).await?;
                return Err(error);
            }
        };
        Ok(Box::pin(stream::unfold(
            Some((responses, capture)),
            |state| async move {
                let (mut responses, mut capture) = state?;
                let response = responses.next().await;
                match response {
                    Some(Ok((message, mut usage))) => {
                        capture.receive(message.as_ref(), usage.as_mut());
                        Some((Ok((message, usage)), Some((responses, capture))))
                    }
                    Some(Err(error)) => {
                        let recorded = capture.finish(Outcome::Failed, Some(&error)).await;
                        Some((Err(recorded.err().unwrap_or(error)), None))
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
    cache_reported: bool,
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
    fn new(model: &Model, config: &ModelConfig) -> Result<Self, ProviderError> {
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
            Purpose::Helper => "helper",
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
                cache_reported: false,
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
            state.usage = counts(usage);
        }
    }
    async fn finish(
        &mut self,
        outcome: Outcome,
        error: Option<&ProviderError>,
    ) -> Result<(), ProviderError> {
        if let Some(state) = self.state.take() {
            state.persist(outcome, error).await
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
        self.request.attempts.push(Attempt {
            index: 1,
            transport: "gdk".into(),
            reused: None,
            headers_us: None,
            response_us: None,
            first_text_us: self.first_text,
            elapsed_us: Some(micros(elapsed)),
            status: None,
            outcome,
            categories: Vec::new(),
            complete_context: false,
            usage: self.usage,
            cache_reported: self.cache_reported,
            reasoning_reported: false,
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
fn counts(usage: &ProviderUsage) -> Option<Usage> {
    Some(Usage {
        input: u64::try_from(usage.usage.input_tokens?).ok()?,
        output: u64::try_from(usage.usage.output_tokens?).ok()?,
        cached_input: usage
            .usage
            .cache_read_input_tokens
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(0),
        reasoning: 0,
    })
}
fn micros(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}
fn millis(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
