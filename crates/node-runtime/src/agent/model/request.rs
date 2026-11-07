//! Apply request recovery and observe the model stream without replaying agent work.
use super::*;
use crate::agent::{canonical, observation::Phase};
use async_trait::async_trait;
use futures::{StreamExt, stream};
use sailry_protocol::{
    SessionId,
    conversation::{
        Usage as Counts,
        observation::{Attempt, Outcome, Request as Observation},
    },
};
use std::time::{Duration, Instant};

pub(super) struct Model {
    inner: Arc<dyn types::Model>,
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
        inner: Arc<dyn types::Model>,
        ingress: &Arc<Ingress>,
        invocation: &Invocation,
        purpose: Purpose,
        stop: &CancellationToken,
        sources: observation::Sources,
        names: names::Registry,
    ) -> Arc<dyn types::Model> {
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
impl types::Model for Model {
    async fn call(
        &self,
        mut request: Request,
        cancel: CancellationToken,
    ) -> Result<ModelStream, Error> {
        let exposure = self.names.get(self.purpose)?;
        request.messages = exposure.messages(&request.messages).into_owned();
        request.tools = exposure.tools(&request.tools).into_owned();
        let capture = Capture::new(self, &request)?;
        let policy = match self.purpose {
            // Summary output stays buffered until an attempt succeeds and is
            // retried at most once; dropping the stream cancels the request.
            Purpose::Compaction => {
                retry::Policy::new(super::summary::RETRIES, super::summary::RETRY_DELAY, true)
            }
            Purpose::Main => retry::Policy::new(5, Duration::from_secs(60), false),
        };
        let responses = retry::stream(
            self.inner.clone(),
            request,
            policy,
            cancel,
            self.stop.clone(),
        );
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
                    Some(Ok(mut event)) => {
                        exposure.restore_event(&mut event);
                        capture.receive(&mut event);
                        if matches!(event, Event::Finish(_)) {
                            return Some((
                                capture
                                    .finish(Outcome::Succeeded, None)
                                    .await
                                    .map(|()| event),
                                None,
                            ));
                        }
                        Some((Ok(event), Some((responses, capture, exposure))))
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
    request: Observation,
    purpose: Purpose,
    started: Instant,
    first_token: Option<u64>,
    usage: Option<Counts>,
    tokens: crate::telemetry::metrics::Tokens,
    metric: Option<(crate::telemetry::Capture, crate::telemetry::metrics::Model)>,
    cache_reported: bool,
    reasoning_reported: bool,
    reasoning_streamed: bool,
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
    async fn record_failure(&mut self, error: &Error) {
        if let Err(recording) = self.finish(Outcome::Failed, Some(error)).await {
            tracing::error!(error = %recording, "model request observation could not be recorded");
        }
    }

    fn new(model: &Model, request: &Request) -> Result<Self, Error> {
        model
            .ingress
            .agents
            .begin_operation(model.turn)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Other,
                    "turn is no longer accepting model requests",
                )
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
        entry.budget = request
            .max_tokens
            .and_then(|value| i64::try_from(value).ok());
        let span = crate::logging::Span::begin(entry, started).cancel_when(model.stop.clone());
        Ok(Self {
            state: Some(State {
                ingress: model.ingress.clone(),
                session: model.session,
                request: Observation {
                    id: identity.call.unwrap().to_string(),
                    turn: model.turn,
                    turn_index: None,
                    model: request.model.clone(),
                    context_limit: request
                        .context_limit
                        .and_then(|value| u32::try_from(value).ok()),
                    timestamp_ms: chrono::Utc::now().timestamp_millis(),
                    boundary: None,
                    attempts: Vec::new(),
                },
                purpose: model.purpose,
                started,
                first_token: None,
                usage: None,
                tokens: Default::default(),
                metric: model.metric.clone().and_then(|metric| {
                    crate::telemetry::capture(model.ingress.profile())
                        .map(|capture| (capture, metric))
                }),
                cache_reported: false,
                reasoning_reported: false,
                reasoning_streamed: false,
                payload: model
                    .sources
                    .capture(&request.system, started)
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
    fn receive(&mut self, event: &mut Event) {
        let state = self.state.as_mut().expect("active request");
        mark(&mut state.first_token, event, state.started);
        earliest(&mut state.first_token, state.payload.first_output());
        state.reasoning_streamed |= reasoning(event);
        if let Event::Usage(usage) = event {
            usage
                .elapsed_ms
                .get_or_insert_with(|| millis(state.started.elapsed()));
            if usage.time_to_first_token_ms.is_none() {
                usage.time_to_first_token_ms = state.first_token.map(|value| value / 1000);
            }
            state.cache_reported = usage.cache_read.is_some();
            state.tokens = crate::telemetry::metrics::Tokens {
                input: usage.input.unwrap_or_default(),
                output: usage.output.unwrap_or_default(),
                cache_read: usage.cache_read.unwrap_or_default(),
                cache_write: usage.cache_write.unwrap_or_default(),
            };
            state.reasoning_reported = usage.reasoning.is_some();
            state.usage = counts(usage);
        }
    }
    async fn finish(&mut self, outcome: Outcome, error: Option<&Error>) -> Result<(), Error> {
        if let Some(state) = self.state.take() {
            let error = error.cloned();
            // A cancelled caller must not cancel the already-owned history append.
            tokio::spawn(async move { state.persist(outcome, error.as_ref()).await })
                .await
                .map_err(|_| {
                    Error::new(
                        ErrorKind::Other,
                        "model request observation could not be recorded",
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
    async fn persist(mut self, outcome: Outcome, error: Option<&Error>) -> Result<(), Error> {
        let elapsed = self.started.elapsed();
        // Raw output may precede every emitted block, e.g. when cancelled
        // during buffered reasoning or tool arguments.
        earliest(&mut self.first_token, self.payload.first_output());
        let outcome = if self.stop.is_cancelled()
            || error.is_some_and(|error| error.kind == ErrorKind::Cancelled)
        {
            Outcome::Cancelled
        } else {
            outcome
        };
        let (categories, complete_context, http) = self.payload.snapshot();
        self.request.attempts.push(Attempt {
            index: 1,
            transport: "model".into(),
            reused: None,
            headers_us: http.map(|receipt| receipt.headers_us),
            response_us: http.map(|receipt| receipt.headers_us),
            first_token_us: self.first_token,
            elapsed_us: Some(micros(elapsed)),
            status: http.map(|receipt| receipt.status),
            outcome,
            categories,
            complete_context,
            usage: self.usage,
            cache_reported: self.cache_reported,
            reasoning_reported: self.reasoning_reported,
            reasoning_streamed: self.reasoning_streamed,
        });
        if let Some(timeline) = self.ingress.agents.timeline(self.request.turn) {
            // Waiting ends at the first token; streamed reasoning, text and
            // tool calls all count as output.
            let wait = self.first_token.unwrap_or_else(|| micros(elapsed));
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
        self.span.entry.first_token_us = self.first_token;
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
                retries: None,
                tokens: self.tokens,
            });
        }
        let phase = match outcome {
            Outcome::Succeeded => crate::logging::Phase::Completed,
            Outcome::Failed => crate::logging::Phase::Failed,
            Outcome::Cancelled => crate::logging::Phase::Cancelled,
        };
        finish_span(&mut self.span, phase, code, error);
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
            Error::new(
                ErrorKind::Other,
                "model request observation could not be recorded",
            )
        })
    }
}
fn finish_span(
    span: &mut crate::logging::Span,
    phase: crate::logging::Phase,
    code: Option<ErrorCode>,
    error: Option<&Error>,
) {
    if matches!(phase, crate::logging::Phase::Failed)
        && code == Some(ErrorCode::Unavailable)
        && error.is_some_and(super::errors::network)
    {
        span.finish_warn(phase, code);
    } else {
        span.finish(phase, code);
    }
}

/// Records the time to first token at the first streamed output message.
///
/// This is a fallback for transports that do not report raw output through [`earliest`].
fn mark(first: &mut Option<u64>, event: &Event, started: Instant) {
    if first.is_none() && output(event) {
        *first = Some(micros(started.elapsed()));
    }
}

/// Prefers a raw output delta observed before the stream emitted its block.
fn earliest(first: &mut Option<u64>, observed: Option<u64>) {
    if let Some(observed) = observed {
        *first = Some(first.map_or(observed, |first| first.min(observed)));
    }
}

/// Streamed model output of any kind marks the time to first token.
fn output(event: &Event) -> bool {
    match event {
        Event::TextDelta { text, .. } => !text.is_empty(),
        Event::ReasoningDelta {
            text, signature, ..
        } => {
            text.as_ref().is_some_and(|text| !text.is_empty())
                || signature
                    .as_ref()
                    .is_some_and(|signature| !signature.is_empty())
        }
        Event::RedactedThinking { data, .. } => !data.is_empty(),
        Event::ToolCall { .. } | Event::ToolCallDelta { .. } => true,
        Event::Usage(_) | Event::Finish(_) => false,
    }
}

/// Streamed reasoning shares the output window measured after the first token.
fn reasoning(event: &Event) -> bool {
    matches!(
        event,
        Event::ReasoningDelta { .. } | Event::RedactedThinking { .. }
    ) && output(event)
}

fn counts(usage: &types::Usage) -> Option<Counts> {
    Some(Counts {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::{Area, Entry, Phase, Span};
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone, Default)]
    struct Levels(Arc<std::sync::Mutex<Vec<tracing::Level>>>);

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Levels {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if event.metadata().level() == &tracing::Level::WARN
                || event.metadata().level() == &tracing::Level::ERROR
            {
                self.0.lock().unwrap().push(*event.metadata().level());
            }
        }
    }

    mod first_token {
        use super::*;

        /// Feeds deltas in order and returns the TTFT after each one.
        fn observe(deltas: &[Event]) -> Vec<Option<u64>> {
            let started = Instant::now() - Duration::from_millis(5);
            let mut first = None;
            deltas
                .iter()
                .map(|delta| {
                    mark(&mut first, delta, started);
                    first
                })
                .collect()
        }

        #[test]
        fn records_tool_call_only_response() {
            let call = Event::ToolCallDelta {
                index: 0,
                id: Some("call".into()),
                name: Some("read_file".into()),
                arguments: String::new(),
            };
            let marks = observe(&[
                Event::Usage(Usage::default()),
                Event::TextDelta {
                    index: 0,
                    text: String::new(),
                },
                call,
            ]);
            assert_eq!(marks[..2], [None, None]);
            assert!(marks[2].is_some_and(|value| value >= 5_000));
        }

        #[test]
        fn records_reasoning_before_text() {
            let marks = observe(&[
                Event::ReasoningDelta {
                    index: 0,
                    text: Some(String::new()),
                    signature: None,
                },
                Event::ReasoningDelta {
                    index: 0,
                    text: Some("Plan".into()),
                    signature: None,
                },
                Event::TextDelta {
                    index: 1,
                    text: "Answer".into(),
                },
            ]);
            assert_eq!(marks[0], None);
            assert!(marks[1].is_some());
            assert_eq!(marks[2], marks[1]);
        }

        #[test]
        fn prefers_raw_delta_before_buffered_block() {
            let mut first = None;
            earliest(&mut first, None);
            assert_eq!(first, None);
            // Reasoning arrived at 2 ms; its complete block was emitted at 900 ms.
            first = Some(900_000);
            earliest(&mut first, Some(2_000));
            assert_eq!(first, Some(2_000));
            // A later observation never moves an earlier first token.
            earliest(&mut first, Some(5_000));
            assert_eq!(first, Some(2_000));
        }

        #[tokio::test]
        async fn keeps_raw_delta_for_tool_only_response() {
            let started = Instant::now() - Duration::from_millis(50);
            let payload = observation::Sources::default().capture("", started);
            // The package decoder saw tool arguments 10 ms in; the complete
            // call reaches the stream only at completion.
            payload.output(started + Duration::from_millis(10));
            let call = Event::ToolCallDelta {
                index: 0,
                id: Some("call".into()),
                name: Some("read_file".into()),
                arguments: String::new(),
            };
            let mut first = None;
            mark(&mut first, &call, started);
            assert!(first.is_some_and(|value| value >= 50_000));
            earliest(&mut first, payload.first_output());
            assert_eq!(first, Some(10_000));
        }
    }

    #[test]
    fn retains_run_and_decode_failures() {
        let levels = Levels::default();
        tracing::subscriber::with_default(
            tracing_subscriber::registry().with(levels.clone()),
            || {
                for error in [
                    Error::new(ErrorKind::Network, "connection failed"),
                    Error::new(ErrorKind::InvalidResponse, "invalid response"),
                    Error::new(ErrorKind::Other, "provider unavailable"),
                ] {
                    let mut span = Span::new(Entry::new(Area::Model, Phase::Begin));
                    finish_span(
                        &mut span,
                        Phase::Failed,
                        Some(ErrorCode::Unavailable),
                        Some(&error),
                    );
                }
                let mut run = Span::new(Entry::new(Area::Run, Phase::Begin));
                run.finish(Phase::Failed, Some(ErrorCode::Unavailable));
            },
        );
        assert_eq!(
            *levels.0.lock().unwrap(),
            [
                tracing::Level::WARN,
                tracing::Level::ERROR,
                tracing::Level::ERROR,
                tracing::Level::ERROR
            ],
        );
    }
}
